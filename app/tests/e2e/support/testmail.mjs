// The local test server (scripts/testmail.sh) as the desktop tests use it. Node only: no
// @wdio/globals here, wdio.conf.mjs (the launcher) imports this file too.
import { execFile, execFileSync, fork } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const here = path.dirname(fileURLToPath(import.meta.url));
export const REPO = path.resolve(here, "../../../..");
const SCRIPT = path.join(REPO, "scripts", "testmail.sh");
const HELPER = path.join(here, "testmail.py");
const PID_FILE = path.join(REPO, "testmail", ".data", "stalwart.pid");

/** Test CA of the server certificate; the e2e build trusts it through this variable (src-tauri/src/e2e.rs). */
export const EXTRA_CA_ENV = "MAIL_APP_E2E_EXTRA_CA";
export const CA_FILE = path.join(REPO, "testmail", ".data", "tls", "ca.pem");
/** `1` lets the e2e build load approved external images from loopback (the counter server). */
export const LOOPBACK_IMAGES_ENV = "MAIL_APP_E2E_LOOPBACK_IMAGES";

/** The test account, from testmail/accounts.env (test values of the local server, not secrets). */
export const ALICE = { email: "alice@vicemail.test", password: "alice-test-pw" };
export const SERVER = { host: "127.0.0.1", imapsPort: 1993, smtpsPort: 1465 };

/** Keychain service of the e2e build (its identifier, tauri.e2e.conf.json). */
export const KEYCHAIN_SERVICE = "org.vicemail.desktop.e2e";

const run = promisify(execFile);

/** Runs scripts/testmail.sh synchronously; for the launcher, before any app is started. */
export function testmailSync(...args) {
  return execFileSync(SCRIPT, args, { cwd: REPO, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
}

/**
 * A fresh server with 10,000 messages for alice (9,959 in INBOX), or the arguments of `seed`
 * given: reset, start, seed. `reset` provisions the new data store through the management API,
 * which has once answered the admin login with HTTP 401 right after the start; another attempt
 * then succeeds. Returns the time taken.
 */
export function prepareTestServer({ attempts = 3, seed = ["10000", "--filler-folder", "INBOX"] } = {}) {
  const started = Date.now();
  for (let attempt = 1; ; attempt += 1) {
    try {
      testmailSync("reset");
      break;
    } catch (e) {
      if (attempt >= attempts) throw e;
      console.log(`testmail.sh reset failed (attempt ${attempt} of ${attempts}), trying again: ${String(e.stderr ?? e.message).trim()}`);
    }
  }
  testmailSync("start");
  testmailSync("seed", ...seed);
  return Date.now() - started;
}

/** Runs scripts/testmail.sh without blocking the worker (the app keeps being driven meanwhile). */
export async function testmail(...args) {
  const { stdout } = await run(SCRIPT, args, { cwd: REPO, encoding: "utf8" });
  return stdout;
}

async function helper(...args) {
  const { stdout } = await run("python3", [HELPER, ...args], { cwd: REPO, encoding: "utf8", timeout: 60000 });
  return JSON.parse(stdout);
}

/** alice's folders on the server: `{ path: { messages, unseen, flags } }` (IMAP STATUS). */
export const serverFolders = () => helper("status");

/** Delivers one message from bob to alice over SMTP submission (STARTTLS, port 1587). */
export const deliverMail = (subject) => helper("send", subject);

/**
 * Creates `folder` in alice's account and appends the .eml files unchanged (IMAP APPEND); with
 * `seen` they are stored as read.
 */
export const appendMessages = (folder, files, { seen = false } = {}) => helper("append", ...(seen ? ["--seen"] : []), folder, ...files);

/** Stage 18: creates `folder` in alice's account directly on the server (another program's folder). */
export const createServerFolder = (folder) => helper("create-folder", folder);

/** Stage 18: deletes alice's `folders` on the server again (deepest first; missing ones skipped). */
export const deleteServerFolders = (folders) => helper("delete-folders", ...folders);

/** SHA-256 and size of every attachment of an .eml file, decoded by Python's email package. */
export const attachmentDigests = (file) => helper("attachments", file);

/** Stage 19: the message in bob's INBOX whose subject contains `mark` (redirect test). */
export const bobMessage = (mark) => helper("bob-message", mark);

/** Stage 20: bob answers alice's message with this subject (In-Reply-To its Message-ID). */
export const replyTo = (subject) => helper("reply-to", subject);

/** Stage 20 (decision 0060): alice's storage limit so that her use is `percent` %, or none. */
export async function setQuota(percent) {
  const args = percent === null ? ["clear", "alice"] : ["fill", "alice", String(percent)];
  const { stdout } = await run("python3", [path.join(REPO, "testmail", "quota.py"), ...args], { cwd: REPO, encoding: "utf8", timeout: 60000 });
  return JSON.parse(stdout);
}

/** Copies every seed message whose X-Vicemail-Seed starts with `prefix` into a new `folder`. */
export const copySeedMessages = (prefix, folder) => helper("copy-seed", prefix, folder);

/** True if the login keychain holds an item of the e2e build's service. */
export function keychainHasEntry() {
  try {
    execFileSync("security", ["find-generic-password", "-s", KEYCHAIN_SERVICE], { stdio: "ignore" });
    return true;
  } catch (e) {
    // 44: errSecItemNotFound. Anything else (locked keychain, missing tool) is a real failure.
    if (e.status === 44) return false;
    throw e;
  }
}

/** PID of the running test server, checked against the process list. */
export function serverPid() {
  const pid = Number(fs.readFileSync(PID_FILE, "utf8").trim());
  const command = execFileSync("ps", ["-p", String(pid), "-o", "command="], { encoding: "utf8" }).trim();
  if (!command.includes("stalwart")) throw new Error(`pid ${pid} is not the test server: ${command}`);
  return pid;
}

/**
 * Pauses and resumes the test server in turns (support/throttle-server.mjs), so a sync takes
 * about (runMs + pauseMs) / runMs times as long. Returns a function that ends it.
 */
export function throttleServer({ runMs, pauseMs }) {
  const child = fork(path.join(here, "throttle-server.mjs"), [String(serverPid()), String(runMs), String(pauseMs)], {
    stdio: "inherit",
  });
  const exited = new Promise((resolve) => child.once("exit", resolve));
  return async () => {
    if (child.exitCode === null) child.send("stop");
    await exited;
  };
}
