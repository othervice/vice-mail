// Proves that the embedded WebDriver and the other e2e helpers are only in test builds
// (decision 0005, docs/ARCHITECTURE.md "Shell").
//
// 1. Positive control: the e2e build (`pnpm build:e2e`) contains every marker string.
// 2. The release build (`CI=true pnpm tauri build --bundles app`) contains none of them.
// 3. `cargo tree` lists tauri-plugin-wdio-webdriver only with `--features e2e`.
// 4. The release app starts, writes to its normal data directory (under a temporary HOME, so the
//    user's real one stays untouched), ignores the MAIL_APP_E2E_* variables and opens no WebDriver port.
//
// Usage: node tests/e2e/check-release.mjs [release-binary] [e2e-binary]
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { DATA_DIR_ENV } from "./support/lifecycle.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const release =
  process.argv[2] ?? path.join(root, "target/release/bundle/macos/Vice Mail.app/Contents/MacOS/mail-app");
const e2e = process.argv[3] ?? path.join(root, "target/debug/mail-app");
const IDENTIFIER = "org.vicemail.desktop";
// Extra root CA for the desktop test's mail account (app/src-tauri/src/e2e.rs); ignored by release builds.
const EXTRA_CA_ENV = "MAIL_APP_E2E_EXTRA_CA";
// External images from loopback addresses (the counter server); only e2e builds know the switch.
const LOOPBACK_IMAGES_ENV = "MAIL_APP_E2E_LOOPBACK_IMAGES";
const PROBE_PORT = 4499;

// Strings that only exist when the plugin or the e2e module are compiled in.
const MARKERS = [
  "TAURI_WEBDRIVER_PORT",
  "wdio-webdriver",
  "WDIO WebDriver plugin initialized",
  "wdioEvalResult",
  "e2e_window_controls",
  "e2e_native_mouse",
  "e2e_opened",
  "e2e_invoked",
  "__e2eCspViolations",
  DATA_DIR_ENV,
  EXTRA_CA_ENV,
  LOOPBACK_IMAGES_ENV,
];

let ok = true;
const fail = (message) => {
  console.log(`  FAIL: ${message}`);
  ok = false;
};

function markersIn(file, hint) {
  if (!fs.existsSync(file)) throw new Error(`missing binary: ${file} (${hint})`);
  const bytes = fs.readFileSync(file);
  return MARKERS.filter((m) => bytes.includes(Buffer.from(m)));
}

const e2eFound = markersIn(e2e, "run `pnpm --dir app build:e2e`");
console.log(`e2e build     ${e2e}\n  markers: ${e2eFound.join(", ") || "none"}`);
if (e2eFound.length !== MARKERS.length) {
  fail("positive control: the e2e build must contain every marker (is target/debug/mail-app a plain debug build? run `pnpm build:e2e`)");
}

const releaseFound = markersIn(release, "run `CI=true pnpm --dir app tauri build --bundles app`");
console.log(`release build ${release}\n  markers: ${releaseFound.join(", ") || "none"}`);
if (releaseFound.length > 0) fail("the release build contains WebDriver or e2e code");

const tree = (features) => {
  try {
    return execFileSync(
      "cargo",
      ["tree", "-p", "mail-app", "-e", "normal", "-i", "tauri-plugin-wdio-webdriver", ...features],
      { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
    );
  } catch (e) {
    return `ERR ${String(e.stderr ?? e.message).trim().split("\n")[0]}`;
  }
};
const withoutFeature = tree([]);
const withFeature = tree(["--features", "e2e"]);
console.log(`cargo tree (default):        ${withoutFeature.trim().split("\n")[0]}`);
console.log(`cargo tree (--features e2e): ${withFeature.trim().split("\n")[0]}`);
if (!withoutFeature.startsWith("ERR") || withFeature.startsWith("ERR")) {
  fail("dependency graph: the plugin must be there only with --features e2e");
}

// --- runtime: start the release app in a throwaway HOME -----------------------------------------

const portOpen = (port) =>
  new Promise((resolve) => {
    const socket = net.connect({ host: "127.0.0.1", port });
    socket.once("connect", () => {
      socket.destroy();
      resolve(true);
    });
    socket.once("error", () => resolve(false));
  });

async function waitFor(check, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (check()) return true;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  return check();
}

const home = fs.mkdtempSync(path.join(os.tmpdir(), "mail-app-release-"));
try {
  const ignoredDir = path.join(home, "e2e-data");
  const appData = path.join(home, "Library", "Application Support", IDENTIFIER);
  const appLogs = path.join(home, "Library", "Logs", IDENTIFIER);
  if (await portOpen(PROBE_PORT)) throw new Error(`port ${PROBE_PORT} is already in use`);

  const child = spawn(release, [], {
    // HOME: Tauri's data and log directories. CFFIXED_USER_HOME: the same for Foundation/WebKit.
    env: { ...process.env, HOME: home, CFFIXED_USER_HOME: home, TAURI_WEBDRIVER_PORT: String(PROBE_PORT), [DATA_DIR_ENV]: ignoredDir, [EXTRA_CA_ENV]: path.join(home, "missing-ca.pem"), [LOOPBACK_IMAGES_ENV]: "1" },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let output = "";
  child.stdout.on("data", (d) => (output += d));
  child.stderr.on("data", (d) => (output += d));
  let exited = null;
  child.once("exit", (code, signal) => (exited = { code, signal }));

  const started = await waitFor(() => fs.existsSync(path.join(appData, "mail.sqlite3")) || exited !== null, 20000);
  // Give the web view a moment, so a crash right after the first frame would show up too.
  await new Promise((resolve) => setTimeout(resolve, 3000));
  const listening = await portOpen(PROBE_PORT);
  const alive = exited === null;
  child.kill("SIGTERM");
  await waitFor(() => exited !== null, 5000);

  const logs = fs.existsSync(appLogs) ? fs.readdirSync(appLogs) : [];
  const log = logs.map((f) => fs.readFileSync(path.join(appLogs, f), "utf8")).join("");
  console.log(`release run   HOME=${home}`);
  console.log(`  database: ${started && fs.existsSync(path.join(appData, "mail.sqlite3")) ? "created" : "missing"}, still running after start: ${alive}`);
  console.log(`  ${DATA_DIR_ENV} ignored: ${!fs.existsSync(ignoredDir)}, WebDriver on ${PROBE_PORT}: ${listening ? "LISTENING" : "nothing"}`);
  console.log(`  log: ${log.trim().split("\n").map((l) => l.replace(/^\S+\s+/, "")).join(" | ") || "(empty)"}`);
  if (!alive) fail(`the release app exited during start (${JSON.stringify(exited)}): ${output.trim()}`);
  if (!fs.existsSync(path.join(appData, "mail.sqlite3"))) fail("the release app created no database in its data directory");
  if (fs.existsSync(ignoredDir)) fail(`the release app used ${DATA_DIR_ENV}`);
  if (listening) fail(`something listens on ${PROBE_PORT} although the release build has no WebDriver`);
  if (!/database ready/.test(log)) fail("the release log does not show a ready database");
  if (/\b(ERROR|WARN)\b/.test(log)) fail("the release log has warnings or errors");
} finally {
  fs.rmSync(home, { recursive: true, force: true });
}

console.log(ok ? "OK: embedded WebDriver and e2e helpers only in the e2e build" : "FAILED");
process.exit(ok ? 0 : 1);
