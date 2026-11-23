// Diagnostic, not in `specs`: why the list scrolls worse after every row of the inbox was rendered
// once (account-sync.e2e.mjs, "reaches every message ..."). Run alone:
//   DIAG_WALK=unfold pnpm test:e2e:run --spec ./tests/e2e/frame.e2e.mjs --spec ./tests/e2e/diag-scroll.e2e.mjs
// DIAG_WALK: "none" (control), "scroll" (render every row, bundles folded), "unfold" (as the test).
//
// Measures, read-only, at each step: DOM nodes, mounted rows, the query cache (found through the
// React root, no app change), and scrolling (support/scroll.mjs) while idle and during a first
// sync of the same account added again.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { browser } from "@wdio/globals";
import { attach, coreCall, SCREENSHOT_DIR } from "./support/app.mjs";
import { measureScroll } from "./support/scroll.mjs";
import { ALICE, SERVER, throttleServer } from "./support/testmail.mjs";

const WALK = process.env.DIAG_WALK ?? "unfold";
/** "1": after removing the account, drop its queries from the cache (diagnostic only). */
const DROP = process.env.DIAG_DROP === "1";
/** "1": after removing the account, reload the page (a fresh JS heap and DOM, same process). */
const RELOAD = process.env.DIAG_RELOAD === "1";
const THROTTLE = { runMs: 5, pauseMs: 745 };
const report = { walk: WALK, drop: DROP, reload: RELOAD, steps: [] };

/** Removes every cached query below `["messages", accountId]` (diagnostic only). */
function dropQueriesOf(accountId) {
  return browser.execute((id) => {
    const root = document.getElementById("root");
    const containerKey = Object.keys(root).find((k) => k.startsWith("__reactContainer$"));
    const stack = [root[containerKey]];
    for (let n = 0; stack.length && n < 20000; n += 1) {
      const fiber = stack.pop();
      if (!fiber) continue;
      const client = fiber.memoizedProps?.client;
      if (client && typeof client.getQueryCache === "function") {
        const before = client.getQueryCache().getAll().length;
        client.removeQueries({ queryKey: ["messages", id] });
        return { before, after: client.getQueryCache().getAll().length };
      }
      if (fiber.sibling) stack.push(fiber.sibling);
      if (fiber.child) stack.push(fiber.child);
    }
    return null;
  }, accountId);
}

const settings = {
  email: ALICE.email,
  displayName: "Alice Archer",
  imap: { host: SERVER.host, port: SERVER.imapsPort, security: "tls", username: ALICE.email },
  smtp: { host: SERVER.host, port: SERVER.smtpsPort, security: "tls", username: ALICE.email },
};

/** DOM size, mounted rows and the query cache, read through the React root (no app change). */
function stats() {
  return browser.execute(() => {
    const root = document.getElementById("root");
    const containerKey = Object.keys(root).find((k) => k.startsWith("__reactContainer$"));
    let client = null;
    const stack = [root[containerKey]];
    for (let n = 0; stack.length && n < 20000 && !client; n += 1) {
      const fiber = stack.pop();
      if (!fiber) continue;
      const candidate = fiber.memoizedProps?.client;
      if (candidate && typeof candidate.getQueryCache === "function") client = candidate;
      if (fiber.sibling) stack.push(fiber.sibling);
      if (fiber.child) stack.push(fiber.child);
    }
    const out = {
      domNodes: document.getElementsByTagName("*").length,
      animations: (() => {
        const all = document.getAnimations();
        const byName = {};
        for (const a of all) {
          const name = a.animationName ?? a.transitionProperty ?? a.constructor.name;
          byName[name] = (byName[name] ?? 0) + 1;
        }
        return { count: all.length, running: all.filter((a) => a.playState === "running").length, byName };
      })(),
      rows: document.querySelectorAll('[data-testid="message-list"] [data-index]').length,
      options: document.querySelectorAll('[data-testid="message-list"] [role="option"]').length,
      query: null,
    };
    if (client) {
      const queries = client.getQueryCache().getAll();
      let pages = 0;
      let items = 0;
      const big = [];
      for (const q of queries) {
        const data = q.state.data;
        let n = 0;
        if (data && Array.isArray(data.pages)) {
          pages += data.pages.length;
          for (const p of data.pages) n += (p.items?.length ?? 0) + (p.rows?.length ?? 0);
        } else if (Array.isArray(data)) n = data.length;
        items += n;
        if (n > 100) big.push({ key: JSON.stringify(q.queryKey), n, observers: q.getObserversCount(), pages: data?.pages?.length ?? null });
      }
      out.query = { queries: queries.length, active: queries.filter((q) => q.getObserversCount() > 0).length, pages, items, big };
    }
    return out;
  });
}

async function step(name, extra = {}) {
  const entry = { name, at: new Date().toISOString(), ...(await stats()), ...extra };
  report.steps.push(entry);
  console.log(`DIAG ${JSON.stringify(entry)}`);
  return entry;
}

const brief = (m) => {
  const s = m.summary ?? {};
  return {
    mode: s.mode ?? m.mode,
    frames: s.frames,
    frameMaxMs: s.frameMaxMs,
    frameP95Ms: s.frameP95Ms,
    longestBlockingMs: s.longestBlockingMs,
    blockingsOver50Ms: s.blockingsOver50Ms,
    framesOver50Ms: s.framesOver50Ms,
    stepCostMaxMs: s.stepCostMaxMs,
    listGrewTimes: s.listGrewTimes,
    listSizes: s.listSizes,
    largestGaps: [...(m.gaps ?? [])].sort((a, b) => b - a).slice(0, 10),
  };
};

async function onlyAccount() {
  const accounts = await coreCall("list_accounts");
  return accounts[0] ?? null;
}

async function waitIdle(timeout = 240000) {
  await browser.waitUntil(
    async () => {
      const a = await onlyAccount();
      return a?.status.state === "idle" && a.status.lastSyncAt !== null;
    },
    { timeout, interval: 500, timeoutMsg: "the sync did not finish" },
  );
}

/** Walks the list from top to end (every page loaded), unfolding bundles if asked. */
async function walk(unfold) {
  const started = Date.now();
  let bundles = 0;
  await browser.execute(() => {
    const list = document.querySelector('[data-testid="message-list"]');
    list.scrollTop = 0;
    list.dispatchEvent(new Event("scroll"));
  });
  await browser.pause(50);
  for (let i = 0; i < 6000; i += 1) {
    const state = await browser.execute((open) => {
      const list = document.querySelector('[data-testid="message-list"]');
      const closed = open ? [...list.querySelectorAll('[data-testid="bundle-row"][data-expanded="false"]')] : [];
      for (const bundle of closed) bundle.click();
      const options = list.querySelectorAll('[role="option"]');
      const last = options[options.length - 1];
      const atEnd = list.scrollTop + list.clientHeight >= list.scrollHeight - 2;
      if (closed.length === 0 && !atEnd) {
        list.scrollTop += Math.max(1, Math.floor(list.clientHeight * 0.8));
        list.dispatchEvent(new Event("scroll"));
      }
      return {
        unfolded: closed.length,
        atEnd,
        last: Number(last?.getAttribute("aria-posinset") ?? 0),
        size: Number(last?.getAttribute("aria-setsize") ?? 0),
        loading: list.textContent.includes("Loading more messages"),
      };
    }, unfold);
    bundles += state.unfolded;
    if (state.unfolded === 0 && state.atEnd && !state.loading && state.size > 0 && state.last === state.size) {
      return { bundles, rows: state.size, ms: Date.now() - started };
    }
    await browser.pause(30);
  }
  throw new Error("the walk did not reach the end");
}

/**
 * DOM churn without the app: 10,200 option elements with names, 30 at a time, as the walk shows
 * them (attached, laid out, replaced). Tells WebKit-level effects from the app's own.
 */
async function domChurn() {
  const started = Date.now();
  for (let i = 0; i < 340; i += 1) {
    await browser.execute((batch) => {
      let box = document.getElementById("diag-churn");
      if (!box) {
        box = document.createElement("div");
        box.id = "diag-churn";
        box.setAttribute("role", "listbox");
        box.setAttribute("aria-label", "churn");
        document.body.append(box);
      }
      box.replaceChildren(
        ...Array.from({ length: 30 }, (_, j) => {
          const el = document.createElement("div");
          el.setAttribute("role", "option");
          el.setAttribute("aria-label", `Unread, Sender ${batch * 30 + j}, Subject number ${batch}, Monday`);
          el.textContent = `Sender ${batch * 30 + j} - Subject number ${batch} - a preview of the message text`;
          return el;
        }),
      );
    }, i);
    await browser.pause(30);
  }
  await browser.execute(() => document.getElementById("diag-churn")?.remove());
  return { ms: Date.now() - started };
}

/** JS allocation without DOM: about as many short-lived objects as the walk's renders. */
async function jsChurn() {
  const started = Date.now();
  for (let i = 0; i < 340; i += 1) {
    await browser.execute(() => {
      const keep = [];
      for (let k = 0; k < 30000; k += 1) keep.push({ id: k, label: `row ${k}`, list: [k, k + 1] });
      window.__diagSink = keep.length;
    });
    await browser.pause(30);
  }
  return { ms: Date.now() - started };
}

/**
 * Starts recording, in the page: core events by type (an extra listener on "core-event") and
 * query fetches by key (a query cache subscription). `stopProbe` returns the numbers.
 */
function startProbe() {
  return browser.execute(async () => {
    const root = document.getElementById("root");
    const containerKey = Object.keys(root).find((k) => k.startsWith("__reactContainer$"));
    const stack = [root[containerKey]];
    let client = null;
    for (let n = 0; stack.length && n < 20000 && !client; n += 1) {
      const fiber = stack.pop();
      if (!fiber) continue;
      const candidate = fiber.memoizedProps?.client;
      if (candidate && typeof candidate.getQueryCache === "function") client = candidate;
      if (fiber.sibling) stack.push(fiber.sibling);
      if (fiber.child) stack.push(fiber.child);
    }
    const probe = { events: {}, fetches: {}, started: performance.now(), timeline: [] };
    window.__diagProbe = probe;
    const internals = window.__TAURI_INTERNALS__;
    const handler = internals.transformCallback((message) => {
      const payload = message.payload ?? message;
      const type = payload?.type ?? "?";
      const size = JSON.stringify(payload).length;
      const e = (probe.events[type] ??= { n: 0, bytes: 0 });
      e.n += 1;
      e.bytes += size;
      if (probe.timeline.length < 400) probe.timeline.push([Math.round(performance.now() - probe.started), `event:${type}`]);
    });
    probe.listenerId = await internals.invoke("plugin:event|listen", { event: "core-event", target: { kind: "Any" }, handler });
    // Rendering updates: a ResizeObserver on a small box that changes its width on every turn
    // of the clock below delivers once per rendering update, at its end.
    probe.renders = [];
    const box = document.createElement("div");
    box.setAttribute("aria-hidden", "true");
    box.style.cssText = "position:fixed;left:0;top:0;height:1px;width:1px;pointer-events:none";
    document.body.append(box);
    probe.box = box;
    let flip = false;
    probe.observer = new ResizeObserver(() => {
      if (probe.renders.length < 400) probe.renders.push(Math.round(performance.now() - probe.started));
    });
    probe.observer.observe(box);
    // Main-thread gaps with their time, to line them up with events and fetches.
    probe.gaps = [];
    const channel = new MessageChannel();
    let last = performance.now();
    probe.running = true;
    channel.port1.onmessage = () => {
      const now = performance.now();
      if (now - last > 30 && probe.gaps.length < 200) probe.gaps.push([Math.round(last - probe.started), Math.round(now - last)]);
      last = now;
      flip = !flip;
      box.style.width = flip ? "2px" : "1px";
      if (probe.running) channel.port2.postMessage(0);
    };
    channel.port2.postMessage(0);
    const pending = new Map();
    probe.unsubscribe = client?.getQueryCache().subscribe((event) => {
      if (event.type !== "updated") return;
      const key = JSON.stringify(event.query.queryKey).slice(0, 80);
      if (event.action.type === "fetch") {
        pending.set(event.query.queryHash, performance.now());
        if (probe.timeline.length < 400) probe.timeline.push([Math.round(performance.now() - probe.started), `fetch:${key}`]);
      }
      if (event.action.type === "success" || event.action.type === "error") {
        const t0 = pending.get(event.query.queryHash);
        pending.delete(event.query.queryHash);
        const f = (probe.fetches[key] ??= { n: 0, ms: 0, maxMs: 0, pages: 0 });
        f.n += 1;
        const ms = t0 === undefined ? 0 : performance.now() - t0;
        f.ms += ms;
        f.maxMs = Math.max(f.maxMs, ms);
        f.pages = event.query.state.data?.pages?.length ?? f.pages;
        if (probe.timeline.length < 400) probe.timeline.push([Math.round(performance.now() - probe.started), `fetched:${key}:${Math.round(ms)}ms`]);
      }
    });
    return Boolean(client);
  });
}

function stopProbe() {
  return browser.execute(async () => {
    const probe = window.__diagProbe;
    if (!probe) return null;
    probe.unsubscribe?.();
    probe.running = false;
    probe.observer?.disconnect();
    probe.box?.remove();
    await window.__TAURI_INTERNALS__.invoke("plugin:event|unlisten", { event: "core-event", eventId: probe.listenerId }).catch(() => undefined);
    for (const f of Object.values(probe.fetches)) {
      f.ms = Math.round(f.ms);
      f.maxMs = Math.round(f.maxMs);
    }
    return { events: probe.events, fetches: probe.fetches, timeline: probe.timeline.slice(0, 120), gaps: probe.gaps, renders: probe.renders };
  });
}

describe("diagnostic: scrolling after every row was rendered", () => {
  before(async () => {
    await attach();
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } });
  });

  after(async () => {
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch(() => undefined);
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, `diag-scroll-${WALK}.json`), JSON.stringify(report, null, 2));
    console.log(`\nDIAG REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("measures before and after the walk, and during a sync of the account added again", async () => {
    await step("start");
    if (process.env.DIAG_PREWAIT === "1") {
      // Only the app's age: 35 s idle before the first account exists.
      await browser.pause(35000);
      await step("waited before adding");
    }
    await coreCall("add_account", { settings, password: ALICE.password });
    await waitIdle();
    await browser.waitUntil(async () => (await stats()).options > 0, { timeout: 20000, timeoutMsg: "no rows" });
    await step("synced");
    // Main-thread gaps with nothing to do (no scrolling) for 3 s.
    await startProbe();
    await browser.pause(3000);
    const quiet = await stopProbe();
    await step("quiet 3 s", { quietGaps: quiet?.gaps });
    if (process.env.DIAG_FINISH === "1") {
      // Experiment: finish the CSS transitions a hidden window never runs to their end.
      await browser.execute(() => {
        for (const animation of document.getAnimations()) animation.finish();
      });
      await step("animations finished");
    }
    if (process.env.DIAG_THEME === "1") {
      // Experiment: switch the theme and back (a full style recalculation and repaint).
      for (const theme of ["dark", "light"]) {
        await coreCall("set_preferences", { preferences: { theme, language: "en" } });
        await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme);
      }
      await browser.pause(300);
    }
    if (process.env.DIAG_ARIAHIDE === "1") {
      // Experiment: take the list out of the accessibility tree (no app change, test only).
      await browser.execute(() => document.querySelector('[data-testid="message-list"]').setAttribute("aria-hidden", "true"));
    }
    await startProbe();
    const idleScroll = brief(await measureScroll({ durationMs: 4000, pxPerFrame: 80 }));
    const idleProbe = await stopProbe();
    await step("scroll idle before", { scroll: idleScroll, idleProbe: { gaps: idleProbe?.gaps, renders: idleProbe?.renders } });

    if (WALK === "wait") {
      // Only time passes (as long as the walk takes): the account's background work goes on.
      await startProbe();
      if (process.env.DIAG_POLL === "1") {
        // The WebDriver asks the page something every 500 ms, as waitUntil loops do.
        for (let i = 0; i < 70; i += 1) {
          await browser.execute(() => document.title);
          await browser.pause(500);
        }
      } else {
        await browser.pause(35000);
      }
      const probe = await stopProbe();
      await step("after waiting", { events: probe?.events, fetches: probe?.fetches, gapsWhileWaiting: probe?.gaps?.length });
    } else if (WALK === "dom" || WALK === "js") {
      const walked = WALK === "dom" ? await domChurn() : await jsChurn();
      await step("after churn", { walked });
      await step("scroll idle after churn", { scroll: brief(await measureScroll({ durationMs: 4000, pxPerFrame: 80 })) });
    } else if (WALK !== "none") {
      const walked = await walk(WALK === "unfold");
      await step("after walk", { walked });
      await step("scroll idle after walk", { scroll: brief(await measureScroll({ durationMs: 4000, pxPerFrame: 80 })) });
    }

    const account = await onlyAccount();
    await coreCall("remove_account", { accountId: account.id });
    await browser.waitUntil(async () => (await coreCall("list_accounts")).length === 0, { timeout: 20000 });
    await browser.pause(1000);
    await step("account removed");
    if (DROP) await step("old queries dropped", { dropped: await dropQueriesOf(account.id) });
    if (RELOAD) {
      await browser.execute(() => {
        window.location.reload();
      });
      await browser.pause(2000);
      await attach();
      await step("page reloaded");
    }

    if (process.env.DIAG_SHOT === "1") await browser.takeScreenshot();
    const stop = throttleServer(THROTTLE);
    try {
      await coreCall("add_account", { settings, password: ALICE.password });
      await browser.waitUntil(
        async () => {
          const s = await stats();
          const size = await browser.execute(
            () => Number(document.querySelector('[data-testid="message-list"] [role="option"]')?.getAttribute("aria-setsize") ?? 0),
          );
          return s.options > 0 && size >= 600;
        },
        { timeout: 60000, interval: 100, timeoutMsg: "the slowed-down sync did not get going" },
      );
      const before = await onlyAccount();
      const probing = await startProbe();
      const measured = await measureScroll({ durationMs: 8000, pxPerFrame: 80 });
      const probe = await stopProbe();
      const after = await onlyAccount();
      await step("scroll during sync", { scroll: brief(measured), stateBefore: before.status.state, stateAfter: after.status.state, probing, probe });
    } finally {
      await stop();
    }
    await waitIdle();
    await step("second sync done");
    assert.ok(true);
  });
});
