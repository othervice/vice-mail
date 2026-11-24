// Diagnostic, not in `specs` (runs only when named): which parts of the database a start reads
// (docs/testing/performance.md, "Desktop-App"). With 100,000 or more messages the app's resident
// memory after a start grew by about the size of the database file; this found the open-reply
// query that scanned the whole message table. PERF=1 gives it the seed of PERF_SEED messages.
//
//   PERF=1 PERF_SEED=100000 E2E_APP=<binary> scripts/e2e-gui.sh --spec ./tests/e2e/diag-start-io.e2e.mjs
//
// Sets up alice and bob as perf.e2e.mjs does, then starts the app with the database out of the
// file cache (support/perf.py evict) and records which pages of mail.sqlite3 are cached at the
// first rows, 3 s and 15 s later (mincore), once with the test server running and once with it
// stopped (no sync). The pages are mapped to tables and indexes afterwards with SQLite's dbstat
// on a copy of the database (DIAG_OUT, default: the screenshots directory).
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { browser } from "@wdio/globals";
import { attach, coreCall, dataDir, SCREENSHOT_DIR } from "./support/app.mjs";
import { quitApp, WEBDRIVER_PORT } from "./support/lifecycle.mjs";
import { ALICE, SERVER, serverFolders, testmail } from "./support/testmail.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const PERF_PY = path.join(here, "support", "perf.py");
const APP = process.env.E2E_APP ?? path.join(here, "../../../target/debug/mail-app");
const OUT = process.env.DIAG_OUT ?? SCREENSHOT_DIR;
const BOB = { email: "bob@vicemail.test", password: "bob-test-pw" };
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const perfPy = (...args) => JSON.parse(execFileSync("python3", [PERF_PY, ...args], { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 }));
const report = { seed: Number(process.env.PERF_SEED || "10000"), runs: [] };
let child = null;

const settings = (email, displayName) => ({
  email,
  displayName,
  imap: { host: SERVER.host, port: SERVER.imapsPort, security: "tls", username: email },
  smtp: { host: SERVER.host, port: SERVER.smtpsPort, security: "tls", username: email },
});

async function rowsShown(timeoutMs = 60000) {
  const deadline = Date.now() + timeoutMs;
  let session = null;
  while (Date.now() < deadline) {
    try {
      if (!session) {
        const res = await fetch(`http://127.0.0.1:${WEBDRIVER_PORT}/session`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ capabilities: {} }),
          signal: AbortSignal.timeout(2000),
        });
        session = (await res.json()).value?.sessionId ?? null;
      }
      if (session) {
        const res = await fetch(`http://127.0.0.1:${WEBDRIVER_PORT}/session/${session}/execute/sync`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ script: "return document.querySelector('[data-testid=\"message-row\"]') !== null;", args: [] }),
          signal: AbortSignal.timeout(2000),
        });
        if ((await res.json()).value === true) break;
      }
    } catch {
      // Not up yet.
    }
    await sleep(20);
  }
  if (session) await fetch(`http://127.0.0.1:${WEBDRIVER_PORT}/session/${session}`, { method: "DELETE" }).catch(() => undefined);
}

async function measuredStart(label) {
  const db = path.join(dataDir(), "data", "mail.sqlite3");
  perfPy("evict", path.join(dataDir(), "data"));
  const before = perfPy("pages", db).resident.length;
  const log = fs.openSync(path.join(dataDir(), `diag-${label}.log`), "a");
  child = spawn(APP, [], { env: { ...process.env, MAIL_APP_E2E_PERF: "1" }, stdio: ["ignore", log, log] });
  fs.closeSync(log);
  const exited = new Promise((resolve) => child.once("exit", resolve));
  const started = Date.now();
  await rowsShown();
  const samples = [{ at: "rows", ms: Date.now() - started, ...perfPy("pages", db) }];
  for (const s of [3, 15]) {
    await sleep(started + s * 1000 - Date.now());
    samples.push({ at: `${s} s`, ms: Date.now() - started, ...perfPy("pages", db) });
  }
  const rss = Number(execFileSync("ps", ["-o", "rss=", "-p", String(child.pid)], { encoding: "utf8" }).trim()) * 1024;
  await quitApp();
  await Promise.race([exited, sleep(20000)]);
  child = null;
  report.runs.push({ label, before, rssMB: Math.round(rss / 1024 / 1024), samples });
  console.log(`${label}: ${samples.map((s) => `${s.at} ${s.resident.length}/${s.pages}`).join(", ")}, rss ${Math.round(rss / 1024 / 1024)} MB`);
}

describe("diagnostic: database pages a start reads", function () {
  this.timeout(60 * 60 * 1000);

  before(async () => {
    await attach();
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } });
    const inbox = (await serverFolders()).INBOX.messages;
    const alice = await coreCall("add_account", { settings: settings(ALICE.email, "Alice Archer"), password: ALICE.password });
    await coreCall("add_account", { settings: settings(BOB.email, "Bob Baker"), password: BOB.password });
    await browser.waitUntil(
      async () => {
        const accounts = await coreCall("list_accounts");
        const boxes = await coreCall("list_mailboxes", { accountId: alice.id });
        return accounts.every((a) => a.status.state === "idle" && a.status.lastSyncAt !== null) && boxes.find((m) => m.path === "INBOX")?.total === inbox;
      },
      { timeout: 60 * 60 * 1000, interval: 2000, timeoutMsg: "the first sync did not finish" },
    );
    await browser.waitUntil(async () => (await coreCall("get_search_index_status")).state === "ready", { timeout: 600000, interval: 2000 });
    await sleep(15000);
    await quitApp();
  });

  after(async () => {
    // Remove the accounts with the binary that added them (keychain).
    if (child) {
      await quitApp().catch(() => undefined);
      child = null;
    }
    await testmail("start").catch(() => undefined);
    const log = fs.openSync(path.join(dataDir(), "diag-cleanup.log"), "a");
    child = spawn(APP, [], { env: process.env, stdio: ["ignore", log, log] });
    fs.closeSync(log);
    await sleep(3000);
    await browser.reloadSession();
    await attach();
    for (const a of await coreCall("list_accounts").catch(() => [])) await coreCall("remove_account", { accountId: a.id }).catch(() => undefined);
    fs.mkdirSync(OUT, { recursive: true });
    fs.writeFileSync(path.join(OUT, `diag-start-io-${report.seed}.json`), JSON.stringify(report));
  });

  it("reads with the server running", async () => {
    await measuredStart("online-1");
    await measuredStart("online-2");
  });

  it("reads with the server stopped", async () => {
    await testmail("stop");
    await measuredStart("offline-1");
    await measuredStart("offline-2");
    fs.mkdirSync(OUT, { recursive: true });
    fs.copyFileSync(path.join(dataDir(), "data", "mail.sqlite3"), path.join(OUT, `diag-start-io-${report.seed}.sqlite3`));
  });
});
