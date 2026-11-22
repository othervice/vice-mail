// Spike D: hostile HTML in sandboxed iframes under WKWebView, driven by WebdriverIO through the
// embedded WebDriver. Fails (exit code != 0) if any vector runs a script, opens a connection to
// the counter server, navigates the app or a frame, or reaches the Tauri interface - and if any
// positive control does not fire (a detector that cannot detect proves nothing).
//
// Results of the last run: e2e/results/sandbox-results.json (not checked in).
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { browser, $, $$ } from "@wdio/globals";
import { counter } from "./counter-server.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const SETTLE_MS = 1500;
const APP_TITLE = "Vice Mail Spike";
/** Commands the test itself sends. Anything else during a hostile set is a leak. */
const TEST_COMMANDS = new Set(["sandbox_report", "sandbox_reset", "e2e_native_mouse"]);
/**
 * Leaks the sandbox + CSP layer cannot stop (WebKit opens a bare TCP connection, no HTTP request).
 * They are expected here and must be removed by the sanitizer in stage 3; any other connection
 * fails the run, and so would an HTTP request from one of these vectors.
 */
const KNOWN_LEAKS = {
  "link-preconnect": { port: 18997, reason: "<link rel=preconnect> ignores CSP in WebKit" },
  "link-target-blank": {
    port: 18999,
    reason: "a click on a target=_blank link preconnects to its host although the sandbox blocks the popup",
  },
};
/** Offset inside a frame that hits the first element (body margin 8px, 16px text). */
const IN_FRAME = { x: 14, y: 17 };

const results = { startedAt: new Date().toISOString(), sets: {}, controls: {}, webdriver: {}, failures: [] };

// ---------------------------------------------------------------------------------------------
// helpers

const snapshot = () => browser.execute(async () => window.__vmSandbox.snapshot());
const clearApp = () => browser.execute(async () => window.__vmSandbox.clear());

async function loadSet(set) {
  await $(`[data-testid="sb-set-${set}"]`).click();
  const status = $('[data-testid="sb-status"]');
  await browser.waitUntil(
    async () => {
      const total = Number(await status.getAttribute("data-total"));
      return (
        (await status.getAttribute("data-set")) === set &&
        total > 0 &&
        Number(await status.getAttribute("data-loaded")) === total
      );
    },
    { timeout: 30000, timeoutMsg: `set ${set}: not all frames fired load` },
  );
}

async function framePoint(frameId) {
  return browser.execute(
    (id, off) => {
      const f = document.querySelector(`[data-frame="${id}"] iframe`);
      if (!f) return null;
      f.scrollIntoView({ block: "center" });
      const r = f.getBoundingClientRect();
      return { x: r.left + off.x, y: r.top + off.y };
    },
    frameId,
    IN_FRAME,
  );
}

/**
 * Real NSEvents into the WKWebView (trusted, with user activation). Only clicks: WebKit ignores
 * mouse-moved events while the window is not key, and an app started over SSH never becomes key.
 * A click on a new spot also delivers a trusted `mouseover` first, so it covers hover vectors.
 */
async function nativeClick(frameId) {
  const p = await framePoint(frameId);
  if (!p) return `frame ${frameId} not found`;
  return browser.execute(
    async (x, y) => {
      try {
        await window.__TAURI_INTERNALS__.invoke("e2e_native_mouse", { kind: "click", x, y, viewportHeight: innerHeight });
        return null;
      } catch (e) {
        return String(e);
      }
    },
    p.x,
    p.y,
  );
}

/**
 * Synthetic events from the parent window into a same-origin frame. This is what the embedded
 * WebDriver itself does for element clicks (`el.click()` from the main frame).
 */
async function syntheticEvent(kind, frameId) {
  return browser.execute(
    (id, k) => {
      const f = document.querySelector(`[data-frame="${id}"] iframe`);
      const d = f && f.contentDocument;
      if (!d) return "opaque frame";
      const el = d.querySelector("a, button, input, div, svg") ?? d.body;
      if (!el) return "no element";
      if (k === "hover") {
        el.dispatchEvent(new MouseEvent("mouseover", { bubbles: true, cancelable: true }));
        return null;
      }
      if (typeof el.click === "function") el.click();
      else el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
      return null;
    },
    frameId,
    kind,
  );
}

const hitsUnder = (hits, prefix) => hits.requests.filter((r) => r.path.startsWith(prefix));

function fail(where, message) {
  results.failures.push(`${where}: ${message}`);
}

/** Each test fails on its own findings; the full list is also in the results file. */
function assertNoFailuresSince(mark) {
  assert.deepEqual(results.failures.slice(mark), []);
}

async function assertAppIntact(where) {
  const handles = await browser.getWindowHandles();
  const url = await browser.getUrl();
  const title = await browser.getTitle();
  if (handles.length !== 1) fail(where, `expected 1 window, found ${handles.length}`);
  if (!url.startsWith("tauri://localhost")) fail(where, `app navigated to ${url}`);
  if (title !== APP_TITLE) fail(where, `app title changed to "${title}"`);
  return { handles: handles.length, url, title };
}

// ---------------------------------------------------------------------------------------------

describe("Spike D: HTML sandbox in WKWebView", function () {
  before(async () => {
    // @wdio/tauri-service tries to re-focus the "active" window before getTitle/$/click through
    // tauri-plugin-wdio (`plugin:wdio|get_window_states`), which this app deliberately does not
    // include; every such command would wait 5 s. An explicit switchToWindow turns that off.
    await browser.switchToWindow(await browser.getWindowHandle());
    // Let the IPC view's start-up self-test finish first: its commands must not land in the
    // command log of the first set (that log also detects an app instance inside a frame).
    await browser.waitUntil(async () => (await $$('[data-testid="checks"] li')).length >= 10, {
      timeout: 20000,
      timeoutMsg: "IPC self-test did not finish",
    });
    await browser.pause(1000);
    await $('[data-testid="nav-sandbox"]').click();
    await $('[data-testid="sb-status"]').waitForExist();
  });

  after(() => {
    results.finishedAt = new Date().toISOString();
    if (results.failures.length > 0) console.log(`\nFAILURES:\n  ${results.failures.join("\n  ")}\n`);
    const dir = path.join(here, "results");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "sandbox-results.json"), JSON.stringify(results, null, 2));
  });

  for (const set of ["A", "B", "N"]) {
    it(`set ${set}: no vector runs a script, connects out, navigates or reaches Tauri`, async () => {
      const where = `set ${set}`;
      const mark = results.failures.length;
      await counter.reset();
      await clearApp();
      await loadSet(set);
      await browser.pause(SETTLE_MS);
      const passive = await counter.hits();

      const first = await snapshot();
      const sameOrigin = first.specs[0].sandbox.includes("allow-same-origin");
      const interactionErrors = [];
      /** frame id -> connections (ports) that appeared while this frame was clicked */
      const clickConnections = {};
      let seen = passive.connections.length;
      for (const spec of first.specs) {
        if (spec.interaction === "none") continue;
        const err = await nativeClick(spec.id);
        if (err) interactionErrors.push(`${spec.id} native click: ${err}`);
        await browser.pause(300);
        if (sameOrigin) {
          const err2 = await syntheticEvent(spec.interaction, spec.id);
          if (err2) interactionErrors.push(`${spec.id} synthetic: ${err2}`);
          await browser.pause(300);
        }
        const now = await counter.hits();
        if (now.connections.length > seen) clickConnections[spec.id] = now.connections.slice(seen).map((c) => c.port);
        seen = now.connections.length;
      }
      await browser.pause(SETTLE_MS);

      const snap = await snapshot();
      const hits = await counter.hits();
      const app = await assertAppIntact(where);
      if (snap.rustError) fail(where, `sandbox_report failed: ${snap.rustError}`);
      for (const e of interactionErrors) fail(where, e);

      // Connections: every TCP connection must be explained by a known leak of a vector that
      // the sanitizer (stage 3) removes; HTTP requests are never acceptable.
      const passivePorts = passive.connections.map((c) => c.port);
      const unexplained = [];
      for (const port of passivePorts) if (!(port === KNOWN_LEAKS["link-preconnect"].port)) unexplained.push(`passive:${port}`);
      for (const [frame, ports] of Object.entries(clickConnections)) {
        const vector = frame.split(":")[1];
        for (const port of ports) if (KNOWN_LEAKS[vector]?.port !== port) unexplained.push(`${frame}:${port}`);
      }
      const late = hits.connections.length - seen;
      if (late > 0) unexplained.push(`${late} after the last click`);
      if (unexplained.length > 0) fail(where, `unexplained connection(s) to the counter: ${unexplained.join(", ")}`);
      if (hits.requests.length > 0) fail(where, `HTTP request(s) reached the counter: ${hits.requests.map((r) => r.path).join(", ")}`);

      const unexpected = snap.rust.commands.filter((c) => !TEST_COMMANDS.has(c));
      if (unexpected.length > 0) fail(where, `unexpected commands reached the core: ${unexpected.join(", ")}`);
      if (snap.rust.breaches.length > 0) fail(where, `breaches: ${snap.rust.breaches.join(", ")}`);

      const rows = [];
      for (const spec of snap.specs) {
        const v = spec.vector;
        const vHits = hitsUnder(hits, `/v/${v}/`).map((r) => `${r.method} ${r.path}`);
        const messages = snap.observed.messages.filter((m) => m.frame === spec.id || m.data?.vmRan === v);
        const insp = snap.inspections.find((i) => i.frame === spec.id) ?? { accessible: false };
        const nested = (insp.nested ?? []).filter((n) => !n.includes("ran=undefined") && n !== "opaque");
        const navs = snap.rust.navigations.filter((n) => n.url.includes(`/v/${v}/`) || (spec.target && n.url === spec.target));
        const intercepts = snap.observed.intercepts.filter((i) => i.frame === spec.id);
        const scriptRan =
          messages.length > 0 || insp.vmRan !== undefined || nested.length > 0 || snap.rust.breaches.some((b) => b.startsWith(v));
        const connections = [
          ...(v === "link-preconnect" ? passivePorts.filter((p) => p === KNOWN_LEAKS["link-preconnect"].port) : []),
          ...(clickConnections[spec.id] ?? []),
        ];
        const row = {
          vector: v,
          kind: spec.kind,
          interaction: spec.interaction,
          hits: vHits,
          connections,
          scriptRan,
          navigations: navs.map((n) => `${n.allowed ? "ALLOWED" : "blocked"} ${n.url}`),
          frameHref: insp.accessible ? insp.href : "(opaque)",
          parentIntercepts: intercepts.map((i) => `${i.type}${i.trusted ? " trusted" : " synthetic"} ${i.href}`),
          targetViaRust: spec.target ? navs.some((n) => n.url === spec.target && !n.allowed) : null,
          frameTauriInternals: insp.accessible ? insp.tauriInternals : "(opaque)",
          nested: insp.nested ?? [],
          color: insp.color ?? null,
          height: insp.height ?? null,
        };
        rows.push(row);

        if (vHits.length > 0) fail(`${where} ${v}`, `counter hits ${vHits.join(", ")}`);
        if (scriptRan) fail(`${where} ${v}`, `script ran: ${JSON.stringify({ messages, vmRan: insp.vmRan, nested })}`);
        if (navs.some((n) => n.allowed)) fail(`${where} ${v}`, `navigation allowed: ${row.navigations.join(", ")}`);
        if (insp.accessible && insp.href !== "about:srcdoc") fail(`${where} ${v}`, `frame navigated to ${insp.href}`);
        if (insp.accessible && insp.tauriInternals !== "undefined")
          fail(`${where} ${v}`, `__TAURI_INTERNALS__ in frame: ${insp.tauriInternals}`);
      }

      // Mail CSS must still apply inside the frame (rendering, not security).
      for (const [vec, color] of [["style-block", "rgb(1, 2, 3)"], ["style-attr", "rgb(4, 5, 6)"]]) {
        const row = rows.find((r) => r.vector === vec);
        if (sameOrigin && row.color !== color) fail(`${where} ${vec}`, `mail CSS not applied: color ${row.color}, expected ${color}`);
      }

      // The plain link: the app must learn the real target through the Rust navigation hook.
      const plain = rows.find((r) => r.vector === "link-plain");
      if (!plain.targetViaRust) fail(where, "link-plain: Rust navigation hook did not see the target");

      results.sets[set] = {
        frame: { sandbox: first.specs[0].sandbox, metaCsp: first.specs[0].metaCsp, parentClicks: first.specs[0].parentClicks },
        passiveConnections: passivePorts,
        clickConnections,
        requests: hits.requests,
        app,
        rust: snap.rust,
        rows,
      };
      assertNoFailuresSince(mark);
    });
  }

  it("controls: every detector fires when the protection is lifted", async () => {
    const where = "controls";
    const mark = results.failures.length;
    await counter.reset();
    await clearApp();
    await loadSet("controls");
    await browser.pause(SETTLE_MS);

    const pointerErr = await nativeClick("controls:ctl-pointer");
    const nativeErr = await nativeClick("controls:ctl-link-native");
    await browser.pause(300);
    const wdErr = await syntheticEvent("click", "controls:ctl-link-wd");
    await browser.pause(SETTLE_MS);
    for (const e of [pointerErr, nativeErr, wdErr]) if (e) fail(where, e);

    const snap = await snapshot();
    const hits = await counter.hits();
    const count = (prefix) => hitsUnder(hits, prefix).length;
    const msgs = (frame) => snap.observed.messages.filter((m) => m.frame === frame).map((m) => m.data);
    const insp = (frame) => snap.inspections.find((i) => i.frame === frame);
    const probe = msgs("controls:ctl-script-probe").find((d) => d && d.probe);
    const soProbe = msgs("controls:ctl-script-so").find((d) => d && d.probe);

    const c = {
      imgAllowedNoMeta: count("/allowed/ctl-img-allowed-nometa/"),
      imgAllowedOpaque: count("/allowed/ctl-img-allowed-opaque/"),
      imgAllowedMeta: count("/allowed/ctl-img-allowed-meta/"),
      imgInherit: count("/ctl/ctl-img-inherit/"),
      cspAttr: count("/allowed/ctl-csp-attr/"),
      cspAttributeSupported: snap.cspAttributeSupported,
      scriptProbe: probe ?? null,
      scriptProbeRawIpc: msgs("controls:ctl-script-probe").filter((d) => d && d.rawIpc).map((d) => d.rawIpc),
      scriptInlineMessages: msgs("controls:ctl-script-inline"),
      scriptSameOrigin: soProbe ?? null,
      scriptSameOriginMarker: insp("controls:ctl-script-so")?.vmRan ?? null,
      scriptSameOriginRawIpc: msgs("controls:ctl-script-so").filter((d) => d && d.rawIpc).map((d) => d.rawIpc),
      breaches: snap.rust.breaches,
      linkNative: count("/allowed/ctl-link-native/"),
      linkNativeNav: snap.rust.navigations.filter((n) => n.url.includes("/allowed/ctl-link-native/")),
      linkSynthetic: count("/allowed/ctl-link-wd/"),
      pointer: msgs("controls:ctl-pointer").filter((d) => d && d.event),
      parentListenerEvents: snap.observed.hovers.length + snap.observed.intercepts.length,
      otherRequests: hits.requests.filter((r) => !r.path.startsWith("/allowed/")).map((r) => r.path),
      app: await assertAppIntact(where),
    };
    results.controls = c;

    if (c.imgAllowedNoMeta < 1) fail(where, "image allowed by the app CSP did not reach the counter (counter or frame broken)");
    if (c.imgAllowedOpaque < 1) fail(where, "image allowed by the app CSP did not load from an opaque frame");
    if (c.imgAllowedMeta !== 0) fail(where, "meta CSP in srcdoc did not block an image the app CSP allows");
    if (c.imgInherit !== 0) fail(where, "srcdoc without meta CSP loaded an image the app CSP forbids (CSP not inherited)");
    if (!probe) fail(where, "script probe in an allow-scripts frame did not report (postMessage detector broken)");
    if (probe && probe.tauriInternals !== "undefined") fail(where, `Tauri injected __TAURI_INTERNALS__ into a sub-frame: ${probe.tauriInternals}`);
    if (c.scriptSameOriginMarker !== "ctl-script-so") fail(where, "marker of a same-origin script not visible (marker detector broken)");
    if (!c.breaches.includes("ctl-script-so#via-parent")) fail(where, "breach via parent not recorded (breach detector broken)");
    const ipcBreaches = c.breaches.filter((b) => /#(own-internals|raw-ipc|webkit-handler)$/.test(b));
    if (ipcBreaches.length > 0) fail(where, `frame reached IPC without the parent: ${ipcBreaches.join(", ")}`);
    if (c.linkNative < 1) fail(where, "native click on an allowed link did not navigate (native clicks broken)");
    if (c.linkSynthetic < 1) fail(where, "synthetic click on an allowed link did not navigate (synthetic clicks broken)");
    for (const type of ["mouseover", "mousedown", "click"])
      if (!c.pointer.some((e) => e.event === type && e.trusted)) fail(where, `native click produced no trusted ${type} inside the frame`);
    assertNoFailuresSince(mark);
  });

  it("webdriver: element lookup inside a sandboxed same-origin frame (informational)", async () => {
    await loadSet("A");
    const out = {};
    const frame = await $('[data-frame="A:link-plain"] iframe');
    try {
      await browser.switchToFrame(frame);
      try {
        out.executeInFrame = await browser.execute(() => `${location.href} tauri=${typeof window.__TAURI_INTERNALS__}`);
      } catch (e) {
        out.executeInFrame = `error: ${String(e.message ?? e).split("\n")[0]}`;
      }
      try {
        const a = await $("a");
        out.findElementInFrame = (await a.isExisting()) ? `found <${await a.getTagName()}>` : "not found";
      } catch (e) {
        out.findElementInFrame = `error: ${String(e.message ?? e).split("\n")[0]}`;
      }
    } catch (e) {
      out.switchToFrame = `error: ${String(e.message ?? e).split("\n")[0]}`;
    } finally {
      await browser.switchToParentFrame().catch(() => browser.switchToFrame(null));
    }
    results.webdriver = out;
  });

});
