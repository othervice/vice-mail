// Stage 15 (decision 0037) in the real app, against a local update server (written for the main
// session; not run by the session that wrote it). The e2e build reads its manifest from
// MAIL_APP_E2E_UPDATE_ENDPOINT (set in wdio.conf.mjs) and trusts the throwaway test key
// (tauri.e2e.conf.json); the files are the core test's fixtures (app/src-tauri/src/updater/).
//
// 1. With the switch off (the default) the app never connects to the update server, not even
//    after a while with Settings → About open: the server counts every TCP connection.
// 2. Positive control: "Check now" connects once and shows version 0.2.0 with its notes as text.
// 3. A manipulated update file is rejected: "failed" with `update_signature_invalid`, nothing
//    installed.
// 4. "Try again" with the genuine file: downloaded, verified, "Restart to install" offered.
// 5. "Restart to install" in a development build: the core refuses to replace itself
//    (`update_install_failed`), the app keeps running.
// 6. No CSP violation on the way.
import assert from "node:assert/strict";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { browser } from "@wdio/globals";
import { attach, cspViolations, openSettings, screenshot, testId } from "./support/app.mjs";

export const UPDATE_PORT = 18996;
const here = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(here, "../../src-tauri/src/updater/fixtures");
const PAYLOAD = fs.readFileSync(path.join(FIXTURES, "update.bin"));
const SIGNATURE = fs.readFileSync(path.join(FIXTURES, "update-0.2.0.sig"), "utf8").trim();
const TARGET = process.platform === "win32" ? "windows-x86_64" : `darwin-${process.arch === "arm64" ? "aarch64" : "x86_64"}`;

const state = { connections: 0, payload: PAYLOAD };

function startServer() {
  const server = http.createServer((req, res) => {
    const base = `http://127.0.0.1:${UPDATE_PORT}`;
    if (req.url === "/latest.json") {
      const manifest = {
        version: "0.2.0",
        notes: "Desktop test release.\n<b>not markup</b>",
        pub_date: "2026-11-23T12:00:00Z",
        platforms: { [TARGET]: { url: `${base}/update.bin`, signature: SIGNATURE } },
      };
      res.writeHead(200, { "Content-Type": "application/json", Connection: "close" });
      res.end(JSON.stringify(manifest));
    } else if (req.url === "/update.bin") {
      res.writeHead(200, { "Content-Type": "application/octet-stream", "Content-Length": state.payload.length, Connection: "close" });
      res.end(state.payload);
    } else {
      res.writeHead(404, { Connection: "close" });
      res.end();
    }
  });
  server.on("connection", () => {
    state.connections += 1;
  });
  return new Promise((resolve) => server.listen(UPDATE_PORT, "127.0.0.1", () => resolve(server)));
}

const updateState = () =>
  browser.execute(() => document.querySelector('[data-testid="updates-state"]')?.getAttribute("data-state") ?? null);

async function waitForState(wanted) {
  await browser.waitUntil(async () => (await updateState()) === wanted, {
    timeout: 20000,
    timeoutMsg: `the update state did not become ${wanted} (is ${String(await updateState())})`,
  });
}

describe("in-app updates (decision 0037)", () => {
  let server;
  before(async () => {
    server = await startServer();
    await attach();
  });
  after(() => {
    server?.close();
  });

  it("connects to nobody while automatic checks are off", async () => {
    await openSettings("about");
    const toggle = await testId("updates-auto-check");
    await toggle.waitForExist({ timeout: 10000 });
    assert.equal(await toggle.getAttribute("aria-checked"), "false");
    await browser.pause(3000);
    assert.equal(state.connections, 0, "no connection without consent or a click");
  });

  it("checks on 'Check now' and shows the update with plain-text notes", async () => {
    await (await testId("updates-check-now")).click();
    await waitForState("available");
    assert.ok(state.connections >= 1, "the click reached the server (positive control)");
    const notes = await (await testId("updates-notes")).getText();
    assert.match(notes, /<b>not markup<\/b>/);
    await screenshot("updates-available");
  });

  it("rejects a manipulated update file", async () => {
    const tampered = Buffer.from(PAYLOAD);
    tampered[0] ^= 0x20;
    state.payload = tampered;
    await (await testId("updates-download")).click();
    await waitForState("failed");
    const code = await browser.execute(() => document.querySelector('[data-testid="updates-failed"]')?.getAttribute("data-error-code"));
    assert.equal(code, "update_signature_invalid");
  });

  it("downloads and verifies the genuine file on 'Try again', then refuses to install a dev build", async () => {
    state.payload = PAYLOAD;
    const failed = await testId("updates-failed");
    await (await failed.$("button")).click();
    await waitForState("ready");
    await screenshot("updates-ready");
    await (await testId("updates-install")).click();
    await waitForState("failed");
    const code = await browser.execute(() => document.querySelector('[data-testid="updates-failed"]')?.getAttribute("data-error-code"));
    assert.equal(code, "update_install_failed");
    assert.deepEqual(await cspViolations(), []);
  });
});
