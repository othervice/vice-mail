// The README screenshots (docs/images/README.md): not part of the normal run. wdio.conf.mjs takes
// this spec only with README_SHOTS=1, and then seeds the test server with the 1,000 messages of the
// rules there (`seed 1000 --no-smtp`) instead of the 10,000 the other specs use. After
// `pnpm --dir app build:e2e`:
//
//   README_SHOTS=1 scripts/e2e-gui.sh --spec ./tests/e2e/readme-shots.e2e.mjs
//
// Writes docs/images/<name>.png for every view of the table in docs/images/README.md except
// windows-inbox.png (needs Windows), with test data only: alice and bob of the local test server,
// default settings, English, the main window at 1440×900 points.
//
// What a picture is: the web view at 2x. WebDriver renders the web content only, so the window
// frame, its shadow and the window buttons are not in it; the buttons are drawn into the page where
// macOS puts them (`e2e_window_controls`) for the main window. `screencapture -l` would include the
// frame but needs the Screen Recording permission, which a launchd job does not have. On a 1x
// screen (a headless Mac mini) a virtual HiDPI display (support/hidpi-display.m) gives the 2x.
// Pictures over about 500 KB are made smaller by support/png.mjs. A picture whose visible text
// shows a home or temp path or the user name of this machine fails its test.
//
// Nothing is sent anywhere: the AI profile points at a cloud provider, but the picture is taken at
// the consent step, before anything would be sent; MCP listens locally and no client connects.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, dataDir, SCREENSHOT_DIR, setConversations, setSidebar, testId } from "./support/app.mjs";
import { startHiDpiDisplay } from "./support/hidpi-display.mjs";
import { optimizePng } from "./support/png.mjs";
import { ALICE, REPO, SERVER } from "./support/testmail.mjs";

const OUT = path.join(REPO, "docs", "images");
const WINDOW = { width: 1440, height: 900 };
const MOD = process.platform === "darwin" ? "Meta" : "Control";
/** docs/images/README.md: keep each file under about 500 KB. */
const MAX_BYTES = 500 * 1000;
/** The second account of the test server (testmail/accounts.env; test values, not secrets). */
const BOB = { email: "bob@vicemail.test", password: "bob-test-pw" };
const report = { images: [], notes: [] };
let display = null;
let alice;

/** Settings of an account on the local test server (implicit TLS for IMAP and SMTP). */
const accountSettings = (email, displayName) => ({
  email,
  displayName,
  imap: { host: SERVER.host, port: SERVER.imapsPort, security: "tls", username: email },
  smtp: { host: SERVER.host, port: SERVER.smtpsPort, security: "tls", username: email },
});

async function setTheme(theme) {
  await coreCall("set_preferences", { preferences: { theme, language: "en" } });
  await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme, {
    timeoutMsg: `theme ${theme} not applied`,
  });
}

/** The window buttons as macOS draws them, at the place the window reports (main window only). */
async function drawWindowButtons() {
  const controls = await coreCall("e2e_window_controls");
  await browser.execute((buttons) => {
    const colours = {
      close: ["#ff5f57", "#e2463f"],
      minimize: ["#febc2e", "#e1a116"],
      zoom: ["#28c840", "#14ae2c"],
    };
    for (const b of buttons) {
      const mark = document.createElement("div");
      mark.dataset.e2eMark = b.name;
      const [fill, edge] = colours[b.name] ?? ["#c8c8c8", "#b0b0b0"];
      const size = Math.min(b.width, b.height);
      Object.assign(mark.style, {
        position: "fixed",
        left: `${b.x + (b.width - size) / 2}px`,
        top: `${b.y + (b.height - size) / 2}px`,
        width: `${size}px`,
        height: `${size}px`,
        borderRadius: "50%",
        background: fill,
        boxShadow: `inset 0 0 0 0.5px ${edge}`,
        zIndex: "2147483647",
        pointerEvents: "none",
      });
      document.body.append(mark);
    }
  }, controls.buttons);
}

const clearWindowButtons = () =>
  browser.execute(() => {
    for (const mark of document.querySelectorAll("[data-e2e-mark]")) mark.remove();
  });

/**
 * Takes the picture of the current window into docs/images/<name>.png: no focus ring of the last
 * synthetic click, transitions finished (a window without animation frames keeps their first
 * frame), the window buttons drawn for the main window.
 */
async function shoot(name, { keepFocus = false, buttons = true } = {}) {
  await browser.execute((keep) => {
    if (!keep && document.activeElement instanceof HTMLElement) document.activeElement.blur();
  }, keepFocus);
  await browser.pause(400);
  await browser.execute(() => {
    for (const animation of document.getAnimations()) animation.finish();
  });
  // Nothing of this machine in the picture: no home or temp path, no user name, wherever such
  // text is on screen, not scrolled out of its pane and not under something opaque.
  const personal = await browser.execute((user) => {
    const pattern = new RegExp(`/Users/|/var/folders/|/private/|\\b${user}\\b`, "gi");
    const alpha = (color) => {
      if (color === "transparent") return 0;
      const parts = color.match(/[\d.]+/g) ?? [];
      return parts.length > 3 ? Number(parts[3]) : 1;
    };
    // Something between the eye and `owner` at this point with a solid background.
    const covered = (hit, owner) => {
      for (let el = hit; el && !el.contains(owner); el = el.parentElement) {
        if (alpha(getComputedStyle(el).backgroundColor) === 1) return true;
      }
      return false;
    };
    // Cut off by a scrolling or clipping ancestor (text scrolled out of a pane).
    const clipped = (r, owner) => {
      for (let el = owner; el && el !== document.body; el = el.parentElement) {
        const style = getComputedStyle(el);
        if (style.overflowX === "visible" && style.overflowY === "visible") continue;
        const box = el.getBoundingClientRect();
        if (r.bottom <= box.top || r.top >= box.bottom || r.right <= box.left || r.left >= box.right) return true;
      }
      return false;
    };
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    const found = [];
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const owner = node.parentElement;
      for (const match of (node.textContent ?? "").matchAll(pattern)) {
        const range = document.createRange();
        range.setStart(node, match.index);
        range.setEnd(node, match.index + match[0].length);
        const shown = [...range.getClientRects()].find((r) => {
          if (r.width <= 0 || r.bottom <= 0 || r.top >= innerHeight || r.right <= 0 || r.left >= innerWidth || clipped(r, owner)) return false;
          const hit = document.elementFromPoint(Math.min(Math.max(r.left + r.width / 2, 0), innerWidth - 1), Math.min(Math.max(r.top + r.height / 2, 0), innerHeight - 1));
          return !hit || !owner || owner.contains(hit) || !covered(hit, owner);
        });
        if (shown) {
          const where = owner?.closest("[data-testid]")?.getAttribute("data-testid") ?? owner?.tagName;
          found.push(`${match[0]} in "${(node.textContent ?? "").trim().slice(0, 60)}" (${String(where)}, top ${Math.round(shown.top)})`);
        }
      }
    }
    return found;
  }, os.userInfo().username);
  if (personal.length > 0) {
    // For a look at what went wrong; tests/e2e/screenshots/ is not checked in.
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    await browser.saveScreenshot(path.join(SCREENSHOT_DIR, `readme-${name}-rejected.png`));
  }
  assert.deepEqual(personal, [], `${name}: visible text shows paths or the user name of this machine`);
  if (buttons) await drawWindowButtons();
  const facts = await browser.execute(() => ({
    hovered: [...document.querySelectorAll(":hover")].at(-1)?.closest("[data-testid]")?.getAttribute("data-testid") ?? null,
    visibility: document.visibilityState,
    width: innerWidth,
    height: innerHeight,
    scale: devicePixelRatio,
    theme: document.documentElement.dataset.theme,
    lang: document.documentElement.lang,
    scrollsSideways: document.documentElement.scrollWidth > document.documentElement.clientWidth,
  }));
  fs.mkdirSync(OUT, { recursive: true });
  const file = path.join(OUT, `${name}.png`);
  await browser.saveScreenshot(file);
  if (buttons) await clearWindowButtons();
  const taken = fs.statSync(file).size;
  const optimized = optimizePng(file, MAX_BYTES, { height: Math.round(facts.height * facts.scale) });
  const size = execFileSync("/usr/bin/sips", ["-g", "pixelWidth", "-g", "pixelHeight", file], { encoding: "utf8" });
  const pixels = size.match(/pixelWidth: (\d+)[\s\S]*pixelHeight: (\d+)/);
  const entry = { name, ...facts, pixels: pixels ? `${pixels[1]}×${pixels[2]}` : null, taken, ...optimized };
  report.images.push(entry);
  console.log(`readme-shots: ${JSON.stringify(entry)}`);
  assert.equal(facts.scrollsSideways, false, `${name}: the window scrolls sideways`);
  assert.ok(optimized.bytes <= MAX_BYTES, `${name}: ${String(optimized.bytes)} bytes`);
  return entry;
}

/** Accessible names of the rendered list rows, to find one and to report what the list showed. */
const rowNames = () =>
  browser.execute(() =>
    [...document.querySelectorAll('[data-testid="message-list"] :is([role="option"], [role="treeitem"])')].map((o) => ({
      id: o.id,
      name: (o.querySelector('[data-testid="row-name"]')?.textContent ?? o.textContent ?? "").trim(),
    })),
  );

/**
 * Scrolls the message list until a row whose accessible name matches `pattern` is rendered,
 * puts it in view and returns its element id (rows are virtualised).
 */
async function revealRow(pattern) {
  for (let i = 0; i < 300; i += 1) {
    const found = (await rowNames()).find((row) => pattern.test(row.name));
    if (found) {
      await browser.execute((id) => document.getElementById(id)?.scrollIntoView({ block: "center" }), found.id);
      await browser.pause(200);
      return found.id;
    }
    const atEnd = await browser.execute(() => {
      const list = document.querySelector('[data-testid="message-list"]');
      const before = list.scrollTop;
      list.scrollTop += list.clientHeight * 0.8;
      list.dispatchEvent(new Event("scroll"));
      return list.scrollTop === before;
    });
    await browser.pause(80);
    if (atEnd) break;
  }
  throw new Error(`no row matches ${pattern}; last rows: ${JSON.stringify((await rowNames()).slice(-5))}`);
}

async function scrollListToTop() {
  await browser.execute(() => {
    const list = document.querySelector('[data-testid="message-list"]');
    list.scrollTop = 0;
    list.dispatchEvent(new Event("scroll"));
  });
  await browser.pause(200);
}

/** Opens the row and waits until the reading pane shows a message with that subject. */
async function openRow(id, subject) {
  await $(`[id="${id}"]`).click();
  await browser.waitUntil(
    () =>
      browser.execute((s) => {
        const pane = document.querySelector('[data-testid="reading-pane"]');
        return Boolean(pane?.textContent?.includes(s));
      }, subject),
    { timeout: 20000, timeoutMsg: `the message "${subject}" did not open` },
  );
  await browser.pause(500);
}

/** The unified inbox ("All inboxes") of the column. */
async function showUnifiedInbox() {
  await setSidebar(true);
  const entry = $('[data-testid="unified-tree"] [role="treeitem"]');
  await entry.waitForExist({ timeout: 20000 });
  await entry.click();
  await browser.waitUntil(async () => (await entry.getAttribute("aria-current")) === "page", { timeoutMsg: "the unified inbox did not open" });
  await testId("message-list").waitForExist({ timeout: 20000 });
  await browser.pause(500);
}

/** Unfolds a bundle row ("Newsletters", "Notifications") unless it is open. */
async function unfoldBundle(name) {
  const id = await revealRow(new RegExp(`^${name}\\b`));
  const expanded = await browser.execute((rowId) => document.getElementById(rowId)?.querySelector('[data-testid="bundle-row"]')?.dataset.expanded, id);
  if (expanded !== "true") {
    await $(`[id="${id}"]`).click();
    await browser.waitUntil(
      () => browser.execute((rowId) => document.getElementById(rowId)?.querySelector('[data-testid="bundle-row"]')?.dataset.expanded === "true", id),
      { timeoutMsg: `the bundle ${name} did not unfold` },
    );
  }
}

/** Folds a bundle row again (the app keeps it unfolded until then). */
async function foldBundle(name) {
  await scrollListToTop();
  const id = await revealRow(new RegExp(`^${name}\\b`));
  const expanded = () => browser.execute((rowId) => document.getElementById(rowId)?.querySelector('[data-testid="bundle-row"]')?.dataset.expanded, id);
  if ((await expanded()) === "true") {
    await $(`[id="${id}"]`).click();
    await browser.waitUntil(async () => (await expanded()) !== "true", { timeoutMsg: `the bundle ${name} did not fold` });
  }
}

/** A real click on the first link of the mail frame whose text matches `text`. */
async function clickFrameLink(text) {
  const point = await browser.execute((wanted) => {
    const frame = document.querySelector('[data-testid="reading-pane"] [data-testid="mail-frame"]');
    const doc = frame?.contentDocument;
    const link = [...(doc?.querySelectorAll("a") ?? [])].find((a) => a.textContent.trim().startsWith(wanted));
    if (!link) return { failure: `no link "${wanted}" in ${JSON.stringify([...(doc?.querySelectorAll("a") ?? [])].map((a) => a.textContent.trim()))}` };
    link.scrollIntoView({ block: "center" });
    const f = frame.getBoundingClientRect();
    const r = link.getClientRects()[0];
    return { x: f.left + r.left + Math.min(r.width / 2, 12), y: f.top + r.top + r.height / 2 };
  }, text);
  if (point.failure) throw new Error(point.failure);
  const viewportHeight = await browser.execute(() => innerHeight);
  return coreCall("e2e_native_mouse", { x: point.x, y: point.y, viewportHeight });
}

/** A handle of a window other than `main` whose page shows `selector`. */
async function switchToWindowWith(selector, main, timeout = 20000) {
  let found = null;
  await browser.waitUntil(
    async () => {
      for (const handle of await browser.getWindowHandles()) {
        if (handle === main) continue;
        await browser.switchToWindow(handle);
        if (await $(selector).isExisting()) {
          found = handle;
          return true;
        }
      }
      await browser.switchToWindow(main);
      return false;
    },
    { timeout, timeoutMsg: `no window shows ${selector}` },
  );
  return found;
}

describe("README screenshots (docs/images)", function () {
  this.timeout(600000);

  before(async () => {
    await attach();
    await setTheme("light");
    if (process.platform === "darwin" && (await browser.execute(() => devicePixelRatio)) < 2) {
      display = await startHiDpiDisplay({ width: 1920, height: 1200, dir: dataDir() });
      report.notes.push(`virtual HiDPI display ${JSON.stringify(display.bounds)}`);
      await browser.setWindowRect(display.bounds.x + 120, display.bounds.y + 120, WINDOW.width, WINDOW.height);
      await browser.waitUntil(async () => (await browser.execute(() => devicePixelRatio)) === 2, {
        timeout: 15000,
        timeoutMsg: "the window did not reach the 2x display",
      });
    }
    await browser.setWindowSize(WINDOW.width, WINDOW.height);
    await browser.waitUntil(
      async () => {
        const size = await browser.execute(() => ({ width: innerWidth, height: innerHeight }));
        return size.width === WINDOW.width && size.height === WINDOW.height;
      },
      { timeout: 10000, timeoutMsg: `the window is not ${WINDOW.width}×${WINDOW.height}` },
    );

    alice = await coreCall("add_account", { settings: accountSettings(ALICE.email, "Alice Archer"), password: ALICE.password });
    const bob = await coreCall("add_account", { settings: accountSettings(BOB.email, "Bob Baker"), password: BOB.password });
    await browser.waitUntil(
      async () => {
        const accounts = await coreCall("list_accounts");
        return accounts.length === 2 && accounts.every((a) => a.status.state === "idle" && a.status.lastSyncAt !== null);
      },
      { timeout: 300000, interval: 500, timeoutMsg: "the first sync did not finish" },
    );
    for (const account of [alice, bob]) {
      await browser.waitUntil(
        async () => {
          const status = await coreCall("get_prefetch_status", { accountId: account.id });
          return status.state === "idle" && status.done === status.total;
        },
        { timeout: 300000, interval: 500, timeoutMsg: `the prefetch of ${account.email} did not finish` },
      );
    }
    // The app's default; stated, because a list of rows per message would look different.
    await setConversations(true);
    // The first start asks once whether to check for updates (decision 0037); answered "No,
    // thanks", the default behaviour, so the card does not cover the column in every picture.
    if (await testId("update-consent-no").isExisting()) {
      await testId("update-consent-no").click();
      await testId("update-consent").waitForExist({ reverse: true });
    }
  });

  after(async () => {
    // Nothing left in the keychain of the test build: clients, profiles, accounts.
    await coreCall("set_ai_enabled", { enabled: false }).catch(() => undefined);
    for (const profile of (await coreCall("get_ai_settings").catch(() => ({ profiles: [] }))).profiles) {
      await coreCall("delete_ai_profile", { profileId: profile.id }).catch(() => undefined);
    }
    await coreCall("set_mcp_enabled", { enabled: false }).catch(() => undefined);
    for (const client of await coreCall("list_mcp_clients").catch(() => [])) {
      await coreCall("revoke_mcp_client", { clientId: client.clientId }).catch(() => undefined);
    }
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    if (display) {
      const removed = await display.stop();
      report.notes.push(removed ? "virtual display removed" : "the virtual display is still online; wake the display to remove it");
    }
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("inbox-light.png and inbox-dark.png: the unified inbox with a newsletter open", async () => {
    await showUnifiedInbox();
    await unfoldBundle("Newsletters");
    // The bundle's rows come in after it unfolds.
    const isNewsletter = (r) => /^(Example News|Rust Users|Vereinsinfo)/.test(r.name);
    let rows = [];
    await browser
      .waitUntil(
        async () => {
          rows = await rowNames();
          return rows.some(isNewsletter);
        },
        { timeout: 10000 },
      )
      .catch(() => undefined);
    const newsletter = rows.find(isNewsletter);
    assert.ok(newsletter, `no newsletter row: ${JSON.stringify(rows.map((r) => r.name))}`);
    report.notes.push(`newsletter: ${newsletter.name}`);
    await $(`[id="${newsletter.id}"]`).click();
    await testId("remote-content-bar").waitForExist({ timeout: 20000 });
    await browser.pause(800);
    await scrollListToTop();
    // docs/DESIGN.md, "Nach den README-Bildern": a row without a preview (older than the prefetch)
    // has its two lines in the middle of the fixed row height.
    const offsets = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid="message-list"] :is([role="option"], [role="treeitem"])')]
        .filter((option) => option.querySelector('[data-testid="row-preview"]') === null && !option.querySelector('[data-testid="bundle-row"]'))
        .map((option) => {
          const row = option.getBoundingClientRect();
          const content = option.querySelector("[aria-hidden] > span")?.getBoundingClientRect();
          return content ? Math.abs(content.top + content.height / 2 - (row.top + row.height / 2)) : null;
        })
        .filter((offset) => offset !== null),
    );
    report.notes.push(`rows without a preview, distance from the middle (px): ${JSON.stringify(offsets)}`);
    assert.ok(offsets.length > 0 && offsets.every((offset) => offset <= 2), JSON.stringify(offsets));
    await shoot("inbox-light");
    await setTheme("dark");
    await shoot("inbox-dark");
    await setTheme("light");
    await foldBundle("Newsletters");
  });

  it("composer.png: formatting, a signature and an attachment in the composer's own window", async () => {
    const main = await browser.getWindowHandle();
    const saved = await coreCall("save_signature", {
      signature: { id: null, name: "Work", html: "<p><strong>Alice Archer</strong><br>Events team · Vice Mail test account</p>" },
    });
    const [identity] = await coreCall("list_identities", { accountId: alice.id });
    await coreCall("set_identity_signature_defaults", {
      identityId: identity.id,
      newSignature: saved.signature.id,
      replySignature: saved.signature.id,
    });
    // An attachment of one's own needs a file dropped on the window (the core takes no path from
    // the page), which WebDriver cannot do: a forward of a seed message keeps the original's.
    // A weekly report with one or two files, a PDF among them if there is one (the seed's
    // attachments are of random kinds).
    const found = await coreCall("search", {
      request: { query: "has:attachment report", scope: { kind: "all" }, cursor: null, limit: 50, typing: false },
    });
    const originals = found.hits.map((h) => h.message).filter((m) => m.accountId === alice.id && m.from?.email !== ALICE.email);
    let draft = null;
    let sender = "";
    for (const wantPdf of [true, false]) {
      for (const original of originals) {
        const candidate = await coreCall("create_draft", { request: { kind: "forward", messageId: original.id, language: "en" } });
        const files = candidate.attachments.map((a) => a.fileName);
        if (files.length > 0 && files.length <= 2 && (!wantPdf || files.some((f) => /\.pdf$/i.test(f)))) {
          draft = candidate;
          sender = (original.from?.name ?? "").split(" ")[0] || "the";
          report.notes.push(`forwarded: ${original.subject} (${files.join(", ")})`);
          break;
        }
        await coreCall("delete_draft", { draftId: candidate.id }).catch(() => undefined);
      }
      if (draft) break;
    }
    assert.ok(draft, "no seed message with one or two attachments to forward");
    draft = await coreCall("save_draft", {
      draftId: draft.id,
      update: {
        identityId: draft.identityId,
        to: [{ name: "Carol Clark", email: "carol@example.org" }],
        cc: draft.cc,
        bcc: draft.bcc,
        subject: draft.subject,
        format: "html",
        bodyHtml:
          `<p>Hi Carol,</p><p>here is ${sender}'s report from last week. Three things for <strong>Thursday</strong>:</p>` +
          "<ul><li>The numbers on page 2 are <strong>final</strong></li><li>Travel expenses still need your sign-off</li><li>Next report comes on <em>Friday</em></li></ul>" +
          "<p>Thanks!</p>",
        bodyText: "",
        includeQuote: draft.includeQuote,
        quotePosition: draft.quotePosition,
        includeSignature: true,
        signatureId: null,
      },
    });
    await coreCall("open_composer_window", { draftId: draft.id });
    await switchToWindowWith('[data-testid="composer-rich-body"]', main);
    // Taller than its first size (820×720), so the signature and the start of the forwarded
    // message show below the text.
    await browser.setWindowSize(820, 900);
    await testId("composer-attachment").waitForExist({ timeout: 20000 });
    await browser.pause(800);
    report.notes.push(`composer window ${JSON.stringify(await browser.getWindowRect())}`);
    // Label, chip and "Cc" of the To line share one middle (they were 4 px apart).
    const middles = await browser.execute(() => {
      const row = document.querySelector('[data-testid="composer-to"]');
      const middle = (el) => {
        const r = el.getBoundingClientRect();
        return Math.round((r.top + r.height / 2) * 10) / 10;
      };
      return {
        label: middle(row.querySelector("label")),
        chip: middle(row.querySelector('[data-testid="recipient-chip"]')),
        cc: middle(row.querySelector('[data-testid="composer-show-cc"]')),
      };
    });
    report.notes.push(`To line middles: ${JSON.stringify(middles)}`);
    assert.ok(Math.abs(middles.label - middles.chip) <= 1 && Math.abs(middles.cc - middles.chip) <= 1, JSON.stringify(middles));
    await shoot("composer", { buttons: false });
    await testId("composer-discard").click();
    await testId("composer-confirm-discard").$("button=Discard").click();
    await browser.switchToWindow(main);
  });

  it("search.png: operators and highlighted results", async () => {
    await showUnifiedInbox();
    // An operator and a word, so the hits are filtered and the word is marked in them. The first
    // with a handful of hits; the counts go to the report.
    const candidates = ["has:attachment report", "from:carol has:attachment", "has:attachment invoice", "from:bob has:attachment"];
    const counts = {};
    for (const candidate of candidates) {
      const page = await coreCall("search", { request: { query: candidate, scope: { kind: "all" }, cursor: null, limit: 100, typing: false } });
      counts[candidate] = page.hits.length;
    }
    report.notes.push(`search counts: ${JSON.stringify(counts)}`);
    const query = candidates.find((c) => counts[c] >= 4 && counts[c] <= 30) ?? candidates[0];
    await browser.keys([MOD, "f"]);
    const input = await testId("search-input");
    await input.waitForExist();
    await input.setValue(query);
    await browser.waitUntil(
      async () => (await browser.execute(() => document.querySelectorAll('[data-testid="message-list"] :is([role="option"], [role="treeitem"])').length)) > 0,
      { timeout: 10000, timeoutMsg: "no search results" },
    );
    await browser.pause(800);
    report.notes.push(`search rows: ${JSON.stringify((await rowNames()).map((r) => r.name))}`);
    report.notes.push(`highlights: ${String(await browser.execute(() => document.querySelectorAll('[data-testid="highlight"]').length))}`);
    await shoot("search");
    await testId("search-close").click().catch(() => undefined);
  });

  it("command-palette.png: the palette over the inbox, filtered", async () => {
    await showUnifiedInbox();
    await browser.keys([MOD, "k"]);
    await testId("command-palette").waitForExist();
    await browser.keys([..."go"]);
    await browser.pause(500);
    report.notes.push(
      `palette: ${JSON.stringify(await browser.execute(() => [...document.querySelectorAll('[data-testid="command-palette"] [role="option"]')].map((o) => o.textContent.trim())))}`,
    );
    report.notes.push(`hovered: ${String(await browser.execute(() => [...document.querySelectorAll(":hover")].at(-1)?.outerHTML.slice(0, 160) ?? null))}`);
    await shoot("command-palette", { keepFocus: true });
    await browser.keys("Escape");
    await testId("command-palette").waitForExist({ reverse: true });
  });

  it("ai-consent.png: what would be sent to a cloud provider, before anything is sent", async () => {
    const settings = await coreCall("get_ai_settings");
    const preset = settings.presets.find((p) => p.kind === "anthropic");
    assert.ok(preset, JSON.stringify(settings.presets));
    const saved = await coreCall("save_ai_profile", {
      profile: {
        id: null,
        name: preset.name,
        kind: preset.kind,
        baseUrl: preset.baseUrl,
        model: "claude-sonnet-5-5",
        structuredOutput: true,
        maxOutputTokens: null,
        key: { type: "keep" },
      },
    });
    const profileId = saved.profiles[0].id;
    for (const aiFunction of ["summary", "reply_suggestion", "writing_help", "search"]) {
      await coreCall("set_ai_function", { aiFunction, profileId });
    }
    await coreCall("set_ai_enabled", { enabled: true });
    await coreCall("set_ai_account_cloud", { accountId: alice.id, allowed: true });
    await showUnifiedInbox();
    await scrollListToTop();
    // A message from a person near the top whose pane offers the AI chips.
    const rows = (await rowNames()).filter((r) => !/^(Newsletters|Notifications)\b/.test(r.name));
    let chosen = null;
    for (const row of rows.slice(0, 10)) {
      await $(`[id="${row.id}"]`).click();
      try {
        await testId("quick-reply-summarize").waitForExist({ timeout: 4000 });
        chosen = row;
        break;
      } catch {
        report.notes.push(`no AI chips for: ${row.name}`);
      }
    }
    assert.ok(chosen, "no message offered the AI chips");
    report.notes.push(`AI on: ${chosen.name}`);
    // The chip opens the panel with the consent step.
    await testId("quick-reply-summarize").click();
    await testId("ai-consent").waitForExist({ timeout: 20000 });
    await testId("ai-consent-exact").waitForExist({ timeout: 20000 });
    const requests = await coreCall("list_ai_requests", { before: null, limit: 10 });
    assert.equal(requests.entries.length, 0, "nothing was sent");
    // The reading pane's toolbar measures its room when it mounts and then on ResizeObserver
    // callbacks, which a window without animation frames (hidden for WebKit) never gets: it would
    // keep the room it had before the panel and the message's own buttons (save, print) narrowed
    // it. It also measures when the account's read-only state changes, so that is switched off and
    // on again, with marking as read on open turned off meanwhile (nothing reaches the server).
    const overflow = () =>
      browser.execute(() => {
        const bar = document.querySelector('[data-testid="reading-pane"] [role="toolbar"]')?.parentElement;
        return bar ? bar.scrollWidth - bar.clientWidth : null;
      });
    const reading = await coreCall("get_reading_preferences");
    await coreCall("set_reading_preferences", { preferences: { ...reading, markReadOnOpen: "never" } });
    await coreCall("set_writes_enabled", { accountId: alice.id, enabled: true });
    await testId("read-only-actions").waitForExist({ reverse: true, timeout: 10000 });
    await coreCall("set_writes_enabled", { accountId: alice.id, enabled: false });
    await testId("read-only-actions").waitForExist({ timeout: 10000 });
    await coreCall("set_reading_preferences", { preferences: reading });
    await browser.pause(300);
    assert.equal(await overflow(), 0, "the toolbar does not fit the reading pane");
    await scrollListToTop();
    // The whole step in view, with its buttons.
    await browser.execute(() => document.querySelector('[data-testid="ai-consent"]')?.scrollIntoView({ block: "end" }));
    await shoot("ai-consent");
    // AI off again (the default), so the panel is gone from the pictures after this one.
    await coreCall("set_ai_enabled", { enabled: false });
    await testId("ai-panel").waitForExist({ reverse: true, timeout: 10000 });
  });

  it("mcp.png: Settings → AI over MCP with access per account and the AI clients", async () => {
    await testId("rail-settings").click().catch(async () => {
      await setSidebar(false);
      await testId("rail-settings").click();
    });
    await testId("settings-tab-mcp").click();
    const toggle = testId("mcp-enabled");
    await toggle.waitForExist();
    if ((await toggle.getAttribute("aria-checked")) !== "true") {
      await toggle.click();
      await testId("mcp-enable-confirm").$('[data-testid="inline-confirm"]').click();
    }
    await browser.waitUntil(async () => (await coreCall("get_mcp_status")).state === "listening", { timeout: 20000, timeoutMsg: "MCP is not listening" });
    await testId(`mcp-share-${alice.id}`).$('[role="radio"][value="read"]').click();
    await browser.waitUntil(
      async () => (await coreCall("get_mcp_settings")).accounts.some((share) => share.accountId === alice.id && share.access === "read"),
      { timeoutMsg: "the share was not stored" },
    );
    await testId("mcp-client-name").setValue("Claude Desktop");
    await testId("mcp-add-submit").click();
    await testId("mcp-created").waitForDisplayed();
    await testId("mcp-created-done").click();
    await testId("mcp-created").waitForDisplayed({ reverse: true });
    await browser.pause(300);
    // Access per account at the top, the AI clients below it. The status above names the helper
    // program and the socket by their paths, which contain the user's name: out of the picture.
    await browser.execute(() => {
      const share = document.querySelector('[data-testid^="mcp-share-"]');
      const block = share?.closest("section") ?? share?.parentElement?.parentElement;
      block?.scrollIntoView({ block: "start" });
    });
    await browser.pause(300);
    report.notes.push(
      `mcp layout: ${JSON.stringify(await browser.execute(() => {
        const block = document.querySelector('[data-testid^="mcp-config-content"]')?.getBoundingClientRect();
        const clients = document.querySelector('[data-testid="mcp-client-name"]')?.getBoundingClientRect();
        return { innerHeight, block: block && { top: block.top, bottom: block.bottom }, nameField: clients && { top: clients.top } };
      }))}`,
    );
    await shoot("mcp");
  });
  // Last: the real click on the link leaves WebKit's pointer over the window, and later pictures
  // would show hover states where it rests.
  it("reading-links.png: the link check of a link whose text and target differ", async () => {
    await showUnifiedInbox();
    const subject = "Your account will be suspended";
    let id;
    try {
      id = await revealRow(new RegExp(subject));
    } catch {
      await scrollListToTop();
      await unfoldBundle("Notifications");
      id = await revealRow(new RegExp(subject));
    }
    await openRow(id, subject);
    await browser.execute((rowId) => document.getElementById(rowId)?.scrollIntoView({ block: "center" }), id);
    await clickFrameLink("https://bank.example/login");
    await testId("link-card").waitForExist({ timeout: 10000 });
    await browser.waitUntil(async () => (await browser.execute(() => document.querySelectorAll('[data-testid="link-card"] [data-warning]').length)) > 0, {
      timeout: 10000,
      timeoutMsg: "the link card shows no warning",
    });
    await shoot("reading-links", { keepFocus: true });
    await browser.keys("Escape");
  });
});
