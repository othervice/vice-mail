// Performance of the real app against the targets of docs/SPEC.md ("Leistungsziele"); results and
// method in docs/testing/performance.md. Not part of the normal run: wdio.conf.mjs takes this spec
// only with PERF=1 and then seeds PERF_SEED messages (default 10,000) into alice's INBOX. After
// building the test app (a release build measures what users get, see performance.md):
//
//   PERF=1 PERF_SEED=100000 E2E_APP=<binary> scripts/e2e-gui.sh --spec ./tests/e2e/perf.e2e.mjs
//
// 1. Setup on the app the service started: alice (the seed) and bob (23 messages) through the core,
//    then wait until both are synced, their bodies prefetched and the search index is ready.
// 2. Start times: the app is quit and started by this spec, PERF_RUNS (default 5) times each way,
//    in turns, after one discarded start. The e2e build's recorder (MAIL_APP_E2E_PERF,
//    src-tauri/src/e2e.rs) notes in the page when the frame, the first message rows and the first
//    message (clicked as soon as the rows are drawn) appear; "usable inbox" is the last of these.
//    - cold: the app binary and every file of the data directory are rewritten without the file
//      cache (support/perf.py, F_NOCACHE), and mincore shows that they are not cached. A rewritten
//      binary is a new file to macOS, which checks a new executable on its first start (as after
//      installing or updating the app): an upper bound.
//    - colddata: only the data directory is rewritten; the binary is one macOS has run before.
//    - warm: right after the previous start, everything cached.
//    What stays warm in every case: macOS, WebKit and the other system libraries (shared with
//    every other program; only a reboot empties that cache, `sudo purge` is not available here).
//    The app's log gives the time to `main` ("starting") and to the open database.
// 3. Idle memory: 60 s after a warm start, RSS and physical footprint of the app and of every
//    process it is responsible for (WebKit's WebContent, Networking, GPU).
// 4. On that app: switching between messages, search to first hits, the visible reaction to
//    actions, scrolling (frame times), memory after each of them, and again after 60 s idle.
// 5. The accounts are removed again with the binary that added them: the keychain lets only that
//    file delete its items (a copy may read them).
//
// Behind the lock screen of a Mac mini nobody sits at, WebKit takes the page for hidden and draws
// no frames. The apps this spec starts load support/keep-rendering.m (DYLD_INSERT_LIBRARIES; test
// tooling, not the app), which turns WebKit's occlusion detection off, and `caffeinate` keeps the
// display awake: the page renders as in front of the user, and `…Painted` times include the frame.
//
// Writes the report to tests/e2e/screenshots/perf-<seed>.json and prints it.
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, dataDir, SCREENSHOT_DIR } from "./support/app.mjs";
import { quitApp, WEBDRIVER_PORT } from "./support/lifecycle.mjs";
import { measureScroll } from "./support/scroll.mjs";
import { ALICE, KEYCHAIN_SERVICE, SERVER, serverFolders } from "./support/testmail.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const PERF_PY = path.join(here, "support", "perf.py");
const KEEP_RENDERING_SOURCE = path.join(here, "support", "keep-rendering.m");
const APP = process.env.E2E_APP ?? path.join(here, "../../../target/debug/mail-app");
const SEED = Number(process.env.PERF_SEED || "10000");
const RUNS = Number(process.env.PERF_RUNS || "5");
const BOB = { email: "bob@vicemail.test", password: "bob-test-pw" };
const BASE = `http://127.0.0.1:${WEBDRIVER_PORT}`;
const IDLE_MS = 60000;

const report = { seed: SEED, runs: RUNS, app: APP, notes: [], environment: {}, setup: {}, starts: [], memory: {}, switching: {}, search: {}, actions: {}, scroll: {} };
let caffeinate = null;
let keepRendering = null;
let current = null; // { child, pid, binary, exited }
/** Whether `browser` talks to the app in `current`. */
let attached = true;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const perfPy = (...args) => JSON.parse(execFileSync("python3", [PERF_PY, ...args], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 }));
const round = (v) => (v === null || v === undefined ? null : Math.round(v * 10) / 10);
const mb = (bytes) => Math.round((bytes / 1024 / 1024) * 10) / 10;

function stats(values) {
  const v = values.filter((x) => typeof x === "number").sort((a, b) => a - b);
  if (v.length === 0) return null;
  const at = (q) => v[Math.min(v.length - 1, Math.floor(q * v.length))];
  return { n: v.length, median: round(v.length % 2 ? v[(v.length - 1) / 2] : (v[v.length / 2 - 1] + v[v.length / 2]) / 2), p95: round(at(0.95)), min: round(v[0]), max: round(v[v.length - 1]) };
}

const accountSettings = (email, displayName) => ({
  email,
  displayName,
  imap: { host: SERVER.host, port: SERVER.imapsPort, security: "tls", username: email },
  smtp: { host: SERVER.host, port: SERVER.smtpsPort, security: "tls", username: email },
});

// --- the app processes this spec starts ---------------------------------------------------------

async function webdriver(method, url, body) {
  const res = await fetch(`${BASE}${url}`, {
    method,
    headers: body ? { "content-type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
    signal: AbortSignal.timeout(15000),
  });
  const json = await res.json();
  if (json.value?.error) throw new Error(`${method} ${url}: ${json.value.error} ${json.value.message}`);
  return json.value;
}

async function waitForDriver(timeoutMs = 60000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      await fetch(`${BASE}/status`, { signal: AbortSignal.timeout(1000) });
      return;
    } catch {
      await sleep(25);
    }
  }
  throw new Error(`the app's WebDriver did not answer within ${timeoutMs} ms`);
}

/** Starts the app binary as a user's double click would start it, with the recorder on. */
function launch(binary, label) {
  const log = fs.openSync(path.join(dataDir(), `perf-${label}.log`), "a");
  const env = { ...process.env, MAIL_APP_E2E_PERF: "1", DYLD_INSERT_LIBRARIES: keepRendering };
  const launchedAt = Date.now();
  const child = spawn(binary, [], { env, stdio: ["ignore", log, log] });
  fs.closeSync(log);
  const exited = new Promise((resolve) => child.once("exit", resolve));
  current = { child, pid: child.pid, binary, exited };
  return launchedAt;
}

/** Waits for the recorder's marks of a start; returns them relative to `launchedAt`. */
async function startMarks(launchedAt, timeoutMs = 120000) {
  await waitForDriver();
  const session = (await webdriver("POST", "/session", { capabilities: {} })).sessionId;
  const deadline = Date.now() + timeoutMs;
  let perf = null;
  try {
    while (Date.now() < deadline) {
      perf = await webdriver("POST", `/session/${session}/execute/sync`, {
        script: "return window.__e2ePerf ? JSON.parse(JSON.stringify({ perf: window.__e2ePerf, visibility: document.visibilityState, view: document.querySelector('[data-testid=\"app-toolbar\"] h1')?.textContent ?? null, size: [innerWidth, innerHeight, devicePixelRatio] })) : null;",
        args: [],
      }).catch(() => null);
      const marks = perf?.perf?.marks ?? {};
      if (marks.openedPainted || (perf?.visibility !== "visible" && marks.opened)) break;
      await sleep(100);
    }
  } finally {
    await webdriver("DELETE", `/session/${session}`).catch(() => undefined);
  }
  if (!perf) throw new Error("the recorder of the e2e build is missing (MAIL_APP_E2E_PERF)");
  const marks = Object.fromEntries(Object.entries(perf.perf.marks).map(([name, m]) => [name, m.wall - launchedAt]));
  return {
    navigationStart: Math.round(perf.perf.timeOrigin - launchedAt),
    marks,
    visibility: perf.visibility,
    view: perf.view,
    size: perf.size,
    opened: perf.perf.opened?.label ?? null,
  };
}

async function quitCurrent() {
  if (!current) return;
  attached = false;
  await quitApp();
  await Promise.race([current.exited, sleep(20000)]);
  if (current.child.exitCode === null && current.child.signalCode === null) {
    current.child.kill("SIGKILL");
    report.notes.push(`pid ${current.pid} did not exit within 20 s after its window closed; killed`);
  }
  current = null;
}

/** Points the spec's WebDriver session (`browser`) at the app this spec started. */
async function attachBrowser() {
  try {
    await browser.reloadSession();
  } catch (e) {
    report.notes.push(`reloadSession failed (${String(e.message).slice(0, 200)}); took a session by hand`);
    browser.sessionId = (await webdriver("POST", "/session", { capabilities: {} })).sessionId;
  }
  attached = true;
  await attach();
}

async function memoryOf(pid) {
  const m = perfPy("memory", String(pid));
  return {
    rssMB: mb(m.rssBytes),
    footprintMB: mb(m.footprintBytes),
    processes: m.processes.map((p) => ({ name: p.name, rssMB: mb(p.rssBytes), footprintMB: p.footprintBytes === null ? null : mb(p.footprintBytes) })),
    // WebKit processes of other programs (or of this one, if the attribution failed).
    otherWebKit: m.otherWebKit.map((p) => ({ name: p.name, responsible: p.responsible, rssMB: mb(p.rssBytes) })),
  };
}

/**
 * What makes up the footprint of the app and of each WebKit process: the first lines of
 * `footprint <pid>` (dirty, clean, reclaimable, regions, category), largest first.
 */
function footprintBreakdown(pid) {
  const m = perfPy("memory", String(pid));
  return Object.fromEntries(
    m.processes.map((p) => {
      let lines;
      try {
        lines = execFileSync("footprint", [String(p.pid)], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] })
          .split("\n")
          .filter((l) => /^\s*[\d.]+ [KMG]?B\s/.test(l))
          .slice(0, 12)
          .map((l) => l.trim().replace(/\s{2,}/g, " | "));
      } catch (e) {
        lines = [`footprint failed: ${String(e.message).slice(0, 120)}`];
      }
      return [p.name, lines];
    }),
  );
}

/** Memory every 10 s while nothing happens for `IDLE_MS`. */
async function idleMemory(pid) {
  const samples = [];
  for (let s = 10; s <= IDLE_MS / 1000; s += 10) {
    await sleep(10000);
    const m = await memoryOf(pid);
    samples.push({ atS: s, rssMB: m.rssMB, footprintMB: m.footprintMB });
    if (s === IDLE_MS / 1000) return { samples, final: m };
  }
  throw new Error("unreachable");
}

/** Memory and page size right after a step (not idle). */
async function phaseMemory(name) {
  const m = await memoryOf(current.pid);
  const page = await browser.execute(() => ({ nodes: document.getElementsByTagName("*").length, frames: document.querySelectorAll("iframe").length }));
  (report.memory.phases ??= []).push({ after: name, rssMB: m.rssMB, footprintMB: m.footprintMB, processes: m.processes, ...page });
}

// --- measurements in the page -------------------------------------------------------------------

/**
 * Runs a trigger in the page and records, for each condition, when the DOM first satisfies it
 * (`dom`, ms after the trigger; a MutationObserver checks after every change) and when the frame
 * after that was drawn (`painted`: a task posted from requestAnimationFrame; null in a page that
 * draws no frames).
 */
function measure(spec) {
  return browser.executeAsync((spec, done) => {
    const now = () => performance.now();
    const setValue = (input, value) => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    };
    const conditions = {
      pane: ({ label }) => {
        const pane = document.querySelector('[data-testid="reading-pane"]');
        const article = pane?.querySelector("article");
        if (!article || article.getAttribute("aria-busy") !== "false") return false;
        const subject = pane.querySelector('[data-testid="reading-subject"]')?.textContent.trim() ?? "";
        const body = pane.querySelector('[data-testid="text-body"], [data-testid="mail-frame"], [data-testid="reading-unavailable"]');
        return body !== null && subject !== "" && label.includes(subject);
      },
      subject: ({ label }) => {
        const subject = document.querySelector('[data-testid="reading-pane"] [data-testid="reading-subject"]')?.textContent.trim() ?? "";
        return subject !== "" && label.includes(subject);
      },
      selected: ({ id }) => document.getElementById(id)?.getAttribute("aria-selected") === "true",
      exists: ({ selector }) => document.querySelector(selector) !== null,
      gone: ({ selector }) => document.querySelector(selector) === null,
      hasIn: ({ id, selector, want }) => {
        const row = document.getElementById(id);
        return row !== null && (row.querySelector(selector) !== null) === want;
      },
      labelHas: ({ id, part, want }) => {
        const label = document.getElementById(id)?.querySelector('[data-testid="row-name"]')?.textContent;
        return label !== undefined && label.split(", ").includes(part) === want;
      },
      changed: ({ id, html }) => {
        const row = document.getElementById(id);
        return row !== null && row.innerHTML !== html;
      },
      // Stage 19: the first row drawn is this message (rows sorted by their place on screen).
      firstRow: ({ id }) => {
        const first = [...document.querySelectorAll('[data-testid="message-list"] [role="option"], [data-testid="message-list"] [role="treeitem"]')].sort(
          (a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top,
        )[0];
        return first !== undefined && first.id.endsWith(`-${String(id)}`);
      },
      // Stage 19: the quick filter's count says this number.
      // (Digits only: the number is written in the UI's language.)
      count: ({ text }) => (document.querySelector('[data-testid="quick-filter-count"]')?.textContent ?? "").replace(/\D/g, "") === text,
      hits: ({ query, needle, previous }) => {
        const list = [...document.querySelectorAll('[data-testid="message-list"]')].find((l) => (l.getAttribute("aria-label") ?? "").includes(query));
        const row = list?.querySelector('[data-testid="message-row"]');
        if (!row) return false;
        const marks = [...row.querySelectorAll('[data-testid="highlight"]')].map((m) => m.textContent.toLowerCase());
        return marks.some((m) => m.includes(needle) || needle.includes(m)) || !previous.includes(row.id);
      },
    };
    let t0 = null;
    const start = () => {
      t0 = now();
    };
    const triggers = {
      // Stage 19: a click on the element `selector` (a menu item, a filter switch).
      clickSelector: ({ selector }) => {
        start();
        document.querySelector(selector).click();
      },
      // Stage 19: typing into the quick filter, one character every `intervalMs`, measured from
      // the last one.
      filterType: ({ text, intervalMs }) => {
        const input = document.querySelector('[data-testid="quick-filter-text"]');
        let typed = 0;
        const step = () => {
          typed += 1;
          if (typed === text.length) start();
          setValue(input, text.slice(0, typed));
          if (typed < text.length) setTimeout(step, intervalMs);
        };
        step();
      },
      click: ({ id }) => {
        start();
        document.getElementById(id).click();
      },
      key: ({ key, shift = false, meta = false }) => {
        const target = document.activeElement ?? document.body;
        start();
        target.dispatchEvent(new KeyboardEvent("keydown", { key, shiftKey: shift, metaKey: meta, bubbles: true, cancelable: true, composed: true }));
        target.dispatchEvent(new KeyboardEvent("keyup", { key, shiftKey: shift, metaKey: meta, bubbles: true, cancelable: true, composed: true }));
      },
      searchSubmit: ({ text }) => {
        const input = document.querySelector('[data-testid="search-input"]');
        start();
        setValue(input, text);
        input.form.requestSubmit();
      },
      searchType: ({ text, intervalMs }) => {
        const input = document.querySelector('[data-testid="search-input"]');
        let typed = 0;
        const step = () => {
          typed += 1;
          if (typed === text.length) start();
          setValue(input, text.slice(0, typed));
          if (typed < text.length) setTimeout(step, intervalMs);
        };
        step();
      },
    };
    const results = {};
    const pending = new Set(spec.until.map((c) => c.name));
    let finished = false;
    const finish = (timedOut) => {
      if (finished) return;
      finished = true;
      observer.disconnect();
      clearInterval(poll);
      clearTimeout(timer);
      done({ results, timedOut: timedOut ? [...pending] : [], visibility: document.visibilityState });
    };
    const painted = (name) => {
      if (document.visibilityState !== "visible") {
        pending.delete(name);
        if (pending.size === 0) finish(false);
        return;
      }
      requestAnimationFrame(() => {
        const channel = new MessageChannel();
        channel.port1.onmessage = () => {
          results[name].painted = Math.round((now() - t0) * 10) / 10;
          pending.delete(name);
          if (pending.size === 0) finish(false);
        };
        channel.port2.postMessage(0);
      });
    };
    const check = () => {
      if (t0 === null || finished) return;
      for (const c of spec.until) {
        if (results[c.name] || !conditions[c.kind](c)) continue;
        results[c.name] = { dom: Math.round((now() - t0) * 10) / 10, painted: null };
        painted(c.name);
      }
    };
    const observer = new MutationObserver(check);
    observer.observe(document, { subtree: true, childList: true, characterData: true, attributes: true });
    // Some conditions change without a DOM mutation of their own (focus); look every 10 ms too.
    const poll = setInterval(check, 10);
    const timer = setTimeout(() => finish(true), spec.timeoutMs ?? 15000);
    triggers[spec.trigger.kind](spec.trigger);
    check();
  }, spec);
}

/** Message rows of the list on screen: id, label, whether the body is here (a preview shows). */
function visibleRows() {
  return browser.execute(() => {
    const list = document.querySelector('[data-testid="message-list"]');
    const view = list.getBoundingClientRect();
    return [...list.querySelectorAll('[data-testid="message-row"]')]
      .filter((el) => {
        const r = el.getBoundingClientRect();
        return r.top >= view.top && r.bottom <= view.bottom;
      })
      .map((el) => ({
        id: el.id,
        label: el.querySelector('[data-testid="row-name"]')?.textContent ?? "",
        local: el.querySelector('[data-testid="row-preview"]') !== null,
        selected: el.getAttribute("aria-selected") === "true",
      }));
  });
}

const scrollList = (by) =>
  browser.execute((delta) => {
    const list = document.querySelector('[data-testid="message-list"]');
    list.scrollTop += delta;
    list.dispatchEvent(new Event("scroll"));
    return list.scrollTop;
  }, by);

function summarize(samples, key = "painted") {
  return { dom: stats(samples.map((s) => s.dom)), painted: stats(samples.map((s) => s[key])), timeouts: samples.filter((s) => s.timedOut).length };
}

const flat = (result, name) => ({ dom: result.results[name]?.dom ?? null, painted: result.results[name]?.painted ?? null, timedOut: result.timedOut.includes(name) });

// --- the spec -------------------------------------------------------------------------------------

describe(`performance with ${SEED} messages (PERF=1)`, function () {
  this.timeout(6 * 60 * 60 * 1000);
  let alice;
  let bob;

  before(async () => {
    report.environment = {
      startedAt: new Date().toISOString(),
      host: os.hostname(),
      cpu: os.cpus()[0]?.model,
      cores: os.cpus().length,
      memoryGB: Math.round(os.totalmem() / 1024 ** 3),
      macos: execFileSync("sw_vers", ["-productVersion"], { encoding: "utf8" }).trim(),
      binaryMB: mb(fs.statSync(APP).size),
      loadBefore: os.loadavg().map((l) => Math.round(l * 100) / 100),
    };
    keepRendering = path.join(dataDir(), "keep-rendering.dylib");
    execFileSync("/usr/bin/clang", ["-dynamiclib", "-fobjc-arc", "-framework", "AppKit", "-framework", "WebKit", "-o", keepRendering, KEEP_RENDERING_SOURCE], {
      stdio: ["ignore", "ignore", "pipe"],
    });
    caffeinate = spawn("/usr/bin/caffeinate", ["-d", "-u", "-t", "21600"], { stdio: "ignore" });

    await attach();
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } });
    const server = await serverFolders();
    report.setup.serverInbox = server.INBOX?.messages ?? null;
    report.setup.serverTotal = Object.values(server).reduce((n, f) => n + f.messages, 0);
    const added = Date.now();
    alice = await coreCall("add_account", { settings: accountSettings(ALICE.email, "Alice Archer"), password: ALICE.password });
    bob = await coreCall("add_account", { settings: accountSettings(BOB.email, "Bob Baker"), password: BOB.password });
    let lastLog = Date.now();
    await browser.waitUntil(
      async () => {
        const accounts = await coreCall("list_accounts");
        const boxes = await coreCall("list_mailboxes", { accountId: alice.id });
        const inbox = boxes.find((m) => m.path === "INBOX");
        if (Date.now() - lastLog > 30000) {
          lastLog = Date.now();
          console.log(`sync: ${accounts.map((a) => `${a.email} ${a.status.state}`).join(", ")}, INBOX ${inbox?.total ?? 0} of ${report.setup.serverInbox}`);
        }
        return accounts.every((a) => a.status.state === "idle" && a.status.lastSyncAt !== null) && inbox?.total === report.setup.serverInbox;
      },
      { timeout: 3 * 60 * 60 * 1000, interval: 2000, timeoutMsg: "the first sync did not finish" },
    );
    report.setup.firstSyncS = Math.round((Date.now() - added) / 1000);
    for (const account of [alice, bob]) {
      await browser.waitUntil(
        async () => {
          const status = await coreCall("get_prefetch_status", { accountId: account.id });
          report.setup[`prefetch-${account.email}`] = status;
          return status.state === "idle" && status.done === status.total;
        },
        { timeout: 3 * 60 * 60 * 1000, interval: 2000, timeoutMsg: `the prefetch of ${account.email} did not finish` },
      );
    }
    report.setup.prefetchDoneS = Math.round((Date.now() - added) / 1000);
    await browser.waitUntil(
      async () => {
        const index = await coreCall("get_search_index_status");
        return index.state === "ready" && index.percent === 100;
      },
      { timeout: 3 * 60 * 60 * 1000, interval: 2000, timeoutMsg: "the search index did not get ready" },
    );
    report.setup.indexReadyS = Math.round((Date.now() - added) / 1000);
    report.setup.local = Object.fromEntries(
      (await coreCall("list_mailboxes", { accountId: alice.id })).map((m) => [m.path, { total: m.total, unread: m.unread }]),
    );
    // Actions change mail on the server: alice writes (bob stays read-only, as new accounts are).
    await coreCall("set_writes_enabled", { accountId: alice.id, enabled: true });
    if (await browser.execute(() => document.querySelector('[data-testid="update-consent-no"]') !== null)) {
      await browser.execute(() => document.querySelector('[data-testid="update-consent-no"]').click());
    }
    // Let the sync and the background jobs after it settle before the first start is measured.
    await sleep(10000);
    const data = path.join(dataDir(), "data");
    report.setup.dataDirMB = mb(
      fs.readdirSync(data, { recursive: true }).reduce((n, f) => {
        const s = fs.statSync(path.join(data, f));
        return n + (s.isFile() ? s.size : 0);
      }, 0),
    );
    report.setup.databaseMB = mb(fs.statSync(path.join(data, "mail.sqlite3")).size);
    console.log(`setup: ${JSON.stringify(report.setup)}`);
    // The app of the service goes; from here on every app is started by this spec.
    await quitApp();
    attached = false;
  });

  after(async () => {
    try {
      // Nothing of the test build left in the keychain: remove the accounts through the core, with
      // the binary that added them (the keychain refuses a copy: "Invalid attempt to change the
      // owner of this item").
      if (current?.binary !== APP) {
        await quitCurrent();
        launch(APP, "cleanup");
        await waitForDriver();
        attached = false;
      }
      if (!attached) await attachBrowser();
      for (const a of await coreCall("list_accounts").catch(() => [])) {
        await coreCall("remove_account", { accountId: a.id }).catch((e) => report.notes.push(`cleanup failed: ${e}`));
      }
      // Stage 22 (decision 0075): on macOS every secret is in one item (the vault), which goes
      // when the last secret goes.
      try {
        execFileSync("security", ["find-generic-password", "-s", KEYCHAIN_SERVICE], { stdio: "ignore" });
        report.notes.push(`the keychain still holds an item of ${KEYCHAIN_SERVICE}`);
      } catch {
        // Not found: removed with the accounts.
      }
    } catch (e) {
      report.notes.push(`cleanup failed: ${String(e.message).slice(0, 300)}`);
    } finally {
      caffeinate?.kill();
      report.environment.loadAfter = os.loadavg().map((l) => Math.round(l * 100) / 100);
      fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
      fs.writeFileSync(path.join(SCREENSHOT_DIR, `perf-${SEED}.json`), JSON.stringify(report, null, 2));
      console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
    }
  });

  it("starts cold and warm to a usable inbox", async () => {
    const coldDir = path.join(dataDir(), "perf-bin");
    fs.mkdirSync(coldDir, { recursive: true });
    const binary = path.join(coldDir, "mail-app");
    perfPy("copy", APP, binary);
    const data = path.join(dataDir(), "data");
    const run = async (kind, index) => {
      if (kind === "cold") perfPy("evict", binary, data);
      if (kind === "colddata") perfPy("evict", data);
      const resident = { binary: perfPy("resident", binary).fraction, data: perfPy("resident", data).fraction };
      const load = os.loadavg()[0];
      const launchedAt = launch(binary, `${kind}-${index}`);
      const marks = await startMarks(launchedAt);
      await quitCurrent();
      const entry = {
        kind,
        index,
        launchedAt,
        residentBefore: { binary: Math.round(resident.binary * 1000) / 1000, data: Math.round(resident.data * 1000) / 1000 },
        load1: Math.round(load * 100) / 100,
        ...marks,
      };
      report.starts.push(entry);
      console.log(`start ${kind} ${index}: ${JSON.stringify(entry.marks)}`);
      await sleep(3000);
    };
    // The first start after the service's app: discarded (a new copy of the binary).
    await run("warmup", 0);
    for (let i = 1; i <= RUNS; i += 1) {
      await run("cold", i);
      await run("warm", i);
      await run("colddata", i);
    }
    // From the app's log (UTC timestamps): `main` reached ("starting") and the database open.
    const lines = fs
      .readdirSync(path.join(dataDir(), "logs"))
      .filter((f) => f.startsWith("app."))
      .flatMap((f) => fs.readFileSync(path.join(dataDir(), "logs", f), "utf8").split("\n"));
    const at = (pattern) => lines.filter((l) => pattern.test(l)).map((l) => Date.parse(l.slice(0, l.indexOf(" "))));
    const mains = at(/ mail_app: starting /);
    const ready = at(/ database ready /);
    for (const entry of report.starts) {
      const main = mains.find((t) => t >= entry.launchedAt - 5);
      const db = ready.find((t) => t >= (main ?? Infinity));
      if (main !== undefined) entry.marks.main = main - entry.launchedAt;
      if (db !== undefined) entry.marks.databaseReady = db - entry.launchedAt;
    }
    const pick = (kind, mark) => stats(report.starts.filter((s) => s.kind === kind).map((s) => s.marks[mark] ?? null));
    report.startSummary = {};
    for (const kind of ["cold", "colddata", "warm"]) {
      report.startSummary[kind] = Object.fromEntries(
        ["main", "databaseReady", "documentStart", "frame", "framePainted", "rows", "rowsPainted", "click", "opened", "openedPainted"].map((m) => [m, pick(kind, m)]),
      );
      report.startSummary[kind].navigationStart = stats(report.starts.filter((s) => s.kind === kind).map((s) => s.navigationStart));
      report.startSummary[kind].load1 = stats(report.starts.filter((s) => s.kind === kind).map((s) => s.load1));
    }
  });

  it("uses little memory when idle after a warm start", async () => {
    const launchedAt = launch(path.join(dataDir(), "perf-bin", "mail-app"), "final");
    report.finalStart = await startMarks(launchedAt);
    report.memory.afterStart = await idleMemory(current.pid);
    report.memory.afterStart.breakdown = footprintBreakdown(current.pid);
    await attachBrowser();
    await phaseMemory("start + 60 s");
  });

  it("switches between messages that are stored here", async () => {
    // The unified inbox the app starts with; the first message is open (the recorder's click).
    const samples = [];
    const seen = new Set();
    for (let round = 0; samples.length < 30 && round < 20; round += 1) {
      const rows = (await visibleRows()).filter((r) => !seen.has(r.id));
      for (const row of rows) {
        if (samples.length >= 30) break;
        seen.add(row.id);
        if (row.selected) continue;
        const result = await measure({
          trigger: { kind: "click", id: row.id },
          until: [
            { name: "selected", kind: "selected", id: row.id },
            { name: "subject", kind: "subject", label: row.label },
            { name: "pane", kind: "pane", label: row.label },
          ],
        });
        samples.push({ local: row.local, selected: flat(result, "selected"), subject: flat(result, "subject"), pane: flat(result, "pane") });
        await sleep(300);
      }
      await scrollList(400);
      await sleep(300);
    }
    const local = samples.filter((s) => s.local);
    report.switching = {
      count: samples.length,
      local: local.length,
      selected: summarize(local.map((s) => s.selected)),
      subject: summarize(local.map((s) => s.subject)),
      pane: summarize(local.map((s) => s.pane)),
      notLocal: summarize(samples.filter((s) => !s.local).map((s) => s.pane)),
      samples,
    };
    await scrollList(-1e9);
    await phaseMemory("switching");
  });

  it("finds the first hits of a search", async () => {
    const open = await measure({ trigger: { kind: "key", key: "/" }, until: [{ name: "field", kind: "exists", selector: '[data-testid="search-input"]' }] });
    report.actions.openSearch = [flat(open, "field")];
    const queries = [
      { kind: "searchSubmit", text: "rechnung", needle: "rechnung" },
      { kind: "searchSubmit", text: "budget", needle: "budget" },
      { kind: "searchSubmit", text: "from:dave", needle: "dave" },
      { kind: "searchSubmit", text: "weihnachtsfeier", needle: "weihnachtsfeier" },
      { kind: "searchType", text: "offsite", needle: "offsite" },
      { kind: "searchType", text: "rech", needle: "rech" },
    ];
    const byQuery = {};
    let previous = [];
    for (let round = 0; round < RUNS; round += 1) {
      for (const q of queries) {
        // Empty field: the folder's list is back.
        await browser.execute(() => {
          const input = document.querySelector('[data-testid="search-input"]');
          Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(input, "");
          input.dispatchEvent(new Event("input", { bubbles: true }));
        });
        await sleep(400);
        const result = await measure({
          trigger: { kind: q.kind, text: q.text, intervalMs: 80 },
          until: [{ name: "hits", kind: "hits", query: q.text, needle: q.needle, previous }],
          timeoutMs: 20000,
        });
        previous = await browser.execute(() => [...document.querySelectorAll('[data-testid="message-list"] [data-testid="message-row"]')].map((r) => r.id));
        const key = `${q.kind === "searchType" ? "typing" : "enter"} ${q.text}`;
        (byQuery[key] ??= []).push(flat(result, "hits"));
        await sleep(300);
      }
    }
    report.search = Object.fromEntries(Object.entries(byQuery).map(([k, v]) => [k, summarize(v)]));
    report.search.samples = byQuery;
    // Close the search: empty field, then Escape closes it.
    await browser.execute(() => {
      const input = document.querySelector('[data-testid="search-input"]');
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(input, "");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await sleep(300);
    // The first Escape closes the list of recent searches, the next one the search.
    await browser.waitUntil(
      async () => {
        const open = await browser.execute(() => {
          const input = document.querySelector('[data-testid="search-input"]');
          input?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
          return input !== null;
        });
        await sleep(200);
        return !open || (await browser.execute(() => document.querySelector('[data-testid="search-input"]') === null));
      },
      { timeout: 5000, timeoutMsg: "the search did not close" },
    );
    await phaseMemory("search");
  });

  it("reacts visibly to actions", async () => {
    // Opening a message changes its row by itself otherwise (read after a moment).
    const reading = await coreCall("get_reading_preferences");
    await coreCall("set_reading_preferences", { preferences: { ...reading, markReadOnOpen: "never" } });
    const samples = { flag: [], unflag: [], unread: [], read: [], pin: [], unpin: [], archive: [], undo: [], palette: [], paletteClose: [] };
    const labelParts = (id) => browser.execute((rowId) => document.getElementById(rowId)?.querySelector('[data-testid="row-name"]')?.textContent.split(", ") ?? [], id);
    for (let round = 0; round < RUNS; round += 1) {
      const rows = (await visibleRows()).filter((r) => r.local);
      const row = rows[1 + (round % Math.max(1, rows.length - 1))] ?? rows[0];
      await browser.execute((id) => document.getElementById(id).click(), row.id);
      await sleep(1500);
      const html = () => browser.execute((id) => document.getElementById(id)?.innerHTML ?? "", row.id);
      const key = async (name, key, until, opts = {}) => {
        const r = await measure({ trigger: { kind: "key", key, ...opts }, until: [{ name: name ?? "x", ...until }] });
        if (name) samples[name].push(flat(r, name));
        await sleep(800);
      };
      // From a known state: not flagged, read (unmeasured).
      if ((await labelParts(row.id)).includes("Flagged")) await key(null, "s", { name: "x", kind: "labelHas", id: row.id, part: "Flagged", want: false });
      if ((await labelParts(row.id)).includes("Unread")) await key(null, "I", { name: "x", kind: "labelHas", id: row.id, part: "Unread", want: false }, { shift: true });
      await key("flag", "s", { kind: "labelHas", id: row.id, part: "Flagged", want: true });
      await key("unflag", "s", { kind: "labelHas", id: row.id, part: "Flagged", want: false });
      await key("unread", "U", { kind: "labelHas", id: row.id, part: "Unread", want: true }, { shift: true });
      await key("read", "I", { kind: "labelHas", id: row.id, part: "Unread", want: false }, { shift: true });
      await key("pin", "p", { kind: "changed", id: row.id, html: await html() });
      await key("unpin", "p", { kind: "changed", id: row.id, html: await html() });
      await key("archive", "e", { kind: "gone", selector: `[id="${row.id}"]` });
      await key("undo", "z", { kind: "exists", selector: `[id="${row.id}"]` });
      await key("palette", "k", { kind: "exists", selector: '[data-testid="command-palette"]' }, { meta: true });
      await key("paletteClose", "Escape", { kind: "gone", selector: '[data-testid="command-palette"]' });
    }
    await coreCall("set_reading_preferences", { preferences: reading });
    await phaseMemory("actions");
    report.actions = { ...report.actions, ...Object.fromEntries(Object.entries(samples).map(([k, v]) => [k, summarize(v)])), samples };
  });

  it("scrolls the inbox", async () => {
    await scrollList(-1e9);
    await sleep(1000);
    // Dropped frames: intervals over 1.5 frames (25 ms) at 60 Hz.
    const scroll = async (pxPerFrame) => {
      const r = await measureScroll({ durationMs: 8000, pxPerFrame });
      return { ...r.summary, fps: round(r.frames.length / 8), framesOver25Ms: r.frames.filter((f) => f > 25).length };
    };
    report.scroll.normal = await scroll(80);
    await scrollList(-1e9);
    await sleep(1000);
    report.scroll.fast = await scroll(240);
    await scrollList(-1e9);
    await phaseMemory("scrolling");
    // Does the web content keep growing with more scrolling (a leak) or level off (caches)?
    report.scroll.repeated = [];
    for (let pass = 1; pass <= 3; pass += 1) {
      await sleep(1000);
      const r = await scroll(240);
      report.scroll.repeated.push({ pass, fps: r.fps, framesOver25Ms: r.framesOver25Ms, rowsScrolled: r.rowsScrolled });
      await scrollList(-1e9);
      await phaseMemory(`scrolling again (${pass})`);
    }
  });

  it("uses little memory when idle after the work", async () => {
    report.memory.afterWork = await idleMemory(current.pid);
    report.memory.afterWork.breakdown = footprintBreakdown(current.pid);
  });

  // Stage 19, task A (decisions 0061, 0062): another order and the quick filter, from the click to
  // the first rows drawn in the new order, in the unified inbox (the start view, conversations)
  // and in alice's inbox (messages). Each order and filter once: a list already loaded comes from
  // the cache, so repeating one would measure the cache. After the memory measurement, so the
  // numbers above stay comparable with earlier runs.
  it("sorts and filters the inbox (stage 19)", async () => {
    // Run alone (`--mochaOpts.grep "stage 19"`), no earlier test has started an app of its own.
    if (!current) {
      const binary = path.join(dataDir(), "perf-bin", "mail-app");
      if (!fs.existsSync(binary)) {
        fs.mkdirSync(path.dirname(binary), { recursive: true });
        perfPy("copy", APP, binary);
      }
      await startMarks(launch(binary, "stage19"));
      await attachBrowser();
      await sleep(3000);
    }
    const unified = { kind: "unified", view: { kind: "all" } };
    const none = { unread: false, flagged: false, attachment: false, contacts: false, tag: null, text: "", monthFirst: false };
    const first = async (scope, order, filter, conversations) => {
      const request = { request: { scope, order, filter }, cursor: null, limit: 1 };
      const page = conversations ? await coreCall("list_view_conversations", request) : await coreCall("list_view_messages", request);
      const item = page.items[0];
      return item ? (conversations ? item.message.id : item.id) : null;
    };
    const openMenu = async () => {
      if ((await $('[data-testid="list-view-menu"]').getAttribute("aria-expanded")) !== "true") await $('[data-testid="list-view-menu"]').click();
      await $('[data-testid="list-view-menu-menu"]').waitForExist({ timeout: 5000 });
    };
    const results = { sort: {}, filter: {}, count: {} };
    // Opened messages stay unread meanwhile: the counts must not move under the measurement.
    const reading = await coreCall("get_reading_preferences");
    await coreCall("set_reading_preferences", { preferences: { ...reading, markReadOnOpen: "never" } });
    // Since stage 18 F the list opens its top message, and a filter keeps the open message in its
    // place (stage 16): the selection goes first, so the first row is the filter's.
    const clearSelection = async () => {
      await browser.execute(() => {
        const list = document.querySelector('[data-testid="message-list"]');
        list?.focus();
        list?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      });
      await sleep(500);
    };
    /** What the bar and the first rows say, for a step that timed out. */
    const shown = () =>
      browser.execute(() => ({
        count: document.querySelector('[data-testid="quick-filter-count"]')?.textContent ?? null,
        first: [...document.querySelectorAll('[data-testid="message-list"] [role="option"], [data-testid="message-list"] [role="treeitem"]')]
          .sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top)
          .slice(0, 3)
          .map((el) => el.id),
      }));
    const conversations = (await coreCall("get_ui_preferences")).conversations;
    for (const [where, scope, viewKey, convs] of [
      ["unified", unified, "unified:all", conversations],
      ["alice inbox", null, null, false],
    ]) {
      let currentScope = scope;
      let currentKey = viewKey;
      if (where === "unified") {
        // All inboxes, whatever view the app opened with (the header's inbox switcher).
        if (!(await $('[data-testid="unified-inbox"]').isExisting())) {
          await browser.execute(() => document.querySelector('[data-testid="inbox-scope"]')?.click());
          await $('[data-testid="inbox-scope-all"]').waitForExist({ timeout: 10000 });
          await browser.execute(() => document.querySelector('[data-testid="inbox-scope-all"]')?.click());
          await $('[data-testid="unified-inbox"]').waitForExist({ timeout: 10000 });
          await sleep(2000);
        }
      }
      if (where === "alice inbox") {
        const inbox = (await coreCall("list_mailboxes", { accountId: alice.id })).find((m) => m.role === "inbox");
        currentScope = { kind: "mailbox", mailboxId: inbox.id };
        currentKey = `mailbox:${String(inbox.id)}`;
        if (conversations) await coreCall("set_ui_preferences", { preferences: { ...(await coreCall("get_ui_preferences")), conversations: false } });
        // Alice's own inbox in the column (her account's tree comes first).
        if ((await $('[data-testid="rail-folders"]').getAttribute("aria-expanded")) !== "true") await $('[data-testid="rail-folders"]').click();
        await browser.waitUntil(
          () => browser.execute(() => document.querySelector('[role="tree"][aria-label^="Folders of"] [data-folder-role="inbox"]') !== null),
          { timeout: 10000, timeoutMsg: "no account tree with an inbox" },
        );
        await browser.execute(() => {
          document.querySelector('[role="tree"][aria-label^="Folders of"] [data-folder-role="inbox"]')?.click();
        });
        await browser.waitUntil(async () => (await $('[data-testid="message-list"]').getAttribute("aria-label"))?.includes("Inbox") ?? false, { timeout: 10000 }).catch(() => undefined);
        await sleep(2000);
      }
      for (const [key, ascending, item] of [
        ["sender", true, "sort-key-sender"],
        ["sender", false, "sort-descending"],
        ["subject", true, "sort-key-subject"],
        ["size", false, "sort-key-size"],
        ["date", true, "sort-key-date"],
      ]) {
        const order = key === "date" ? { key, ascending: true } : { key, ascending };
        // "Oldest first": the date key first, then the direction.
        if (key === "date") {
          await openMenu();
          await $('[data-testid="sort-key-date"]').click();
          await sleep(1500);
        }
        const id = await first(currentScope, order, none, convs);
        await openMenu();
        const result = await measure({
          trigger: { kind: "clickSelector", selector: `[data-testid="${key === "date" ? "sort-ascending" : item}"]` },
          until: [{ name: "rows", kind: "firstRow", id }],
          timeoutMs: 20000,
        });
        results.sort[`${where}: ${key} ${ascending ? "asc" : "desc"}`] = flat(result, "rows");
        await sleep(1000);
      }
      // Back to newest first through the menu (the window keeps the orders it has read): the
      // key is date already, so the direction.
      await openMenu();
      await $('[data-testid="sort-descending"]').click();
      await sleep(1500);
      await coreCall("set_list_order", { view: currentKey, order: null });
      for (const [name, filter, selector] of [
        ["unread", { ...none, unread: true }, "quick-filter-unread"],
        ["unread + attachment", { ...none, unread: true, attachment: true }, "quick-filter-attachment"],
        ["unread + attachment + flagged", { ...none, unread: true, attachment: true, flagged: true }, "quick-filter-flagged"],
      ]) {
        const id = await first(currentScope, { key: "date", ascending: false }, filter, convs);
        const count = await coreCall("count_list_view", { request: { scope: currentScope, order: { key: "date", ascending: false }, filter } });
        const until = [{ name: "count", kind: "count", text: String(count) }];
        if (id !== null) until.push({ name: "rows", kind: "firstRow", id });
        await clearSelection();
        const result = await measure({ trigger: { kind: "clickSelector", selector: `[data-testid="${selector}"]` }, until, timeoutMs: 20000 });
        results.filter[`${where}: ${name}`] = id !== null ? flat(result, "rows") : null;
        results.count[`${where}: ${name} (${String(count)})`] = flat(result, "count");
        if (result.timedOut.length > 0) (results.shown ??= {})[`${where}: ${name}`] = { expected: id, count, ...(await shown()) };
        await sleep(1000);
      }
      await $('[data-testid="quick-filter-reset"]').click();
      await sleep(1500);
      for (const text of ["rechnung", "budget"]) {
        const filter = { ...none, text };
        const id = await first(currentScope, { key: "date", ascending: false }, filter, convs);
        const count = await coreCall("count_list_view", { request: { scope: currentScope, order: { key: "date", ascending: false }, filter } });
        await clearSelection();
        const result = await measure({
          trigger: { kind: "filterType", text, intervalMs: 80 },
          until: [
            ...(id !== null ? [{ name: "rows", kind: "firstRow", id }] : []),
            { name: "count", kind: "count", text: String(count) },
          ],
          timeoutMs: 20000,
        });
        results.filter[`${where}: typing ${text} (incl. 150 ms pause)`] = id !== null ? flat(result, "rows") : null;
        results.count[`${where}: typing ${text} (${String(count)})`] = flat(result, "count");
        if (result.timedOut.length > 0) (results.shown ??= {})[`${where}: typing ${text}`] = { expected: id, count, ...(await shown()) };
        await $('[data-testid="quick-filter-reset"]').click();
        await sleep(1500);
      }
    }
    if (conversations) await coreCall("set_ui_preferences", { preferences: { ...(await coreCall("get_ui_preferences")), conversations: true } });
    await coreCall("set_reading_preferences", { preferences: reading });
    report.stage19 = results;
    console.log(`stage 19: ${JSON.stringify(results)}`);
  });
});
