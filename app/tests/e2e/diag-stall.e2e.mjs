// Diagnostic, not in `specs`: is the page's main thread descheduled (by the OS or WebKit's
// process throttling) once the hidden test window has been idle for a while? A synchronous loop
// with no yields measures the longest wall-clock gap between two of its iterations: JavaScript
// cannot pause inside it, so a long gap means the thread did not run.
import { browser } from "@wdio/globals";
import { attach } from "./support/app.mjs";

const busyLoop = (ms) =>
  browser.execute((duration) => {
    const start = performance.now();
    let last = start;
    let longest = 0;
    let over50 = 0;
    let iterations = 0;
    while (last - start < duration) {
      const now = performance.now();
      const gap = now - last;
      if (gap > longest) longest = gap;
      if (gap > 50) over50 += 1;
      last = now;
      iterations += 1;
    }
    return { longestGapMs: Math.round(longest), gapsOver50Ms: over50, iterations, visibility: document.visibilityState };
  }, ms);

/** The same loop, allocating (and keeping a window of) small objects: a GC pause shows as a gap. */
const allocLoop = (ms) =>
  browser.execute((duration) => {
    const start = performance.now();
    let last = start;
    let longest = 0;
    let over50 = 0;
    const ring = new Array(200000);
    let i = 0;
    while (last - start < duration) {
      ring[i % ring.length] = { i, s: `row ${i}`, a: [i, i + 1, i + 2] };
      i += 1;
      const now = performance.now();
      const gap = now - last;
      if (gap > longest) longest = gap;
      if (gap > 50) over50 += 1;
      last = now;
    }
    return { longestGapMs: Math.round(longest), gapsOver50Ms: over50, allocations: i };
  }, ms);

/**
 * Scrolls a plain scrollable box (no app code: 2,000 static rows) one step per 16.7 ms on a
 * MessageChannel clock, as support/scroll.mjs does in a hidden window, and records the gaps
 * between the clock's turns.
 */
const synthScroll = (ms) => {
  return browser.executeAsync((duration, done) => {
    let box = document.getElementById("diag-scroller");
    if (!box) {
      box = document.createElement("div");
      box.id = "diag-scroller";
      box.setAttribute("aria-hidden", "true");
      box.style.cssText = "position:fixed;left:0;top:0;width:400px;height:500px;overflow:auto;z-index:9999;background:white";
      for (let i = 0; i < 2000; i += 1) {
        const row = document.createElement("div");
        row.style.cssText = "height:44px;border-bottom:1px solid #ddd";
        row.textContent = `Row ${i} - sender - subject of the message`;
        box.append(row);
      }
      document.body.append(box);
    }
    box.scrollTop = 0;
    const gaps = [];
    const channel = new MessageChannel();
    const start = performance.now();
    let last = start;
    let next = start;
    let steps = 0;
    let stepMax = 0;
    channel.port1.onmessage = () => {
      const now = performance.now();
      if (now - last > 4) gaps.push(Math.round(now - last));
      if (now - start > duration) {
        box.remove();
        const sorted = [...gaps].sort((a, b) => b - a);
        done({ steps, stepMaxMs: Math.round(stepMax), longestGapMs: sorted[0] ?? 0, gapsOver50Ms: gaps.filter((g) => g > 50).length, largest: sorted.slice(0, 8) });
        return;
      }
      if (now >= next) {
        const t0 = performance.now();
        box.scrollTop += 80;
        if (box.scrollTop >= box.scrollHeight - box.clientHeight - 1) box.scrollTop = 0;
        box.dispatchEvent(new Event("scroll"));
        box.firstElementChild.getBoundingClientRect();
        stepMax = Math.max(stepMax, performance.now() - t0);
        steps += 1;
        next += 1000 / 60;
        if (next <= now) next = now + 1000 / 60;
      }
      last = performance.now();
      channel.port2.postMessage(0);
    };
    channel.port2.postMessage(0);
  }, ms);
};

/**
 * Like a virtual list without any app code: every 16.7 ms the box's 30 rows are replaced by new
 * ones (role, name and text with umlauts, accents and emoji, as in the seed's subjects).
 */
const churnScroll = (ms) =>
  browser.executeAsync((duration, done) => {
    const box = document.createElement("div");
    box.setAttribute("role", "listbox");
    box.setAttribute("aria-label", "diag");
    box.style.cssText = "position:fixed;left:0;top:0;width:500px;height:600px;overflow:hidden;z-index:9999;background:white";
    document.body.append(box);
    const names = ["Zoë Ångström", "François Lefèvre", "Grüße 🧾", "Peter O'Brien", "Smith, John", "Bob Baker ✓", "Ärger mit Öl", "日本語のテスト"];
    let n = 0;
    const fill = () => {
      box.replaceChildren(
        ...Array.from({ length: 30 }, (_, j) => {
          const row = document.createElement("div");
          row.setAttribute("role", "option");
          const who = names[(n + j) % names.length];
          row.setAttribute("aria-label", `Unread, ${who}, Subject ${n + j} 📎, Monday`);
          row.textContent = `${who} — Re: Angebot Nr. ${n + j} für die Präsentation 🎉 Überblick`;
          row.style.cssText = "height:20px;white-space:nowrap;overflow:hidden";
          return row;
        }),
      );
      n += 30;
    };
    const gaps = [];
    const channel = new MessageChannel();
    const start = performance.now();
    let last = start;
    let next = start;
    channel.port1.onmessage = () => {
      const now = performance.now();
      if (now - last > 4) gaps.push(Math.round(now - last));
      if (now - start > duration) {
        box.remove();
        const sorted = [...gaps].sort((a, b) => b - a);
        done({ rows: n, longestGapMs: sorted[0] ?? 0, gapsOver50Ms: gaps.filter((g) => g > 50).length, largest: sorted.slice(0, 8) });
        return;
      }
      if (now >= next) {
        fill();
        box.firstElementChild.getBoundingClientRect();
        next += 1000 / 60;
        if (next <= now) next = now + 1000 / 60;
      }
      last = performance.now();
      channel.port2.postMessage(0);
    };
    channel.port2.postMessage(0);
  }, ms);

describe("diagnostic: main-thread stalls of the hidden window", () => {
  before(async () => {
    await attach();
  });

  it("measures a busy loop right after the start and after 40 s idle", async () => {
    await browser.setTimeout({ script: 60000 });
    console.log(`DIAG fresh synthetic scroll ${JSON.stringify(await synthScroll(4000))}`);
    console.log(`DIAG fresh churn ${JSON.stringify(await churnScroll(4000))}`);
    console.log(`DIAG fresh busy ${JSON.stringify(await busyLoop(2000))}`);
    console.log(`DIAG fresh alloc ${JSON.stringify(await allocLoop(3000))}`);
    await browser.pause(40000);
    console.log(`DIAG idle busy ${JSON.stringify(await busyLoop(2000))}`);
    for (let i = 0; i < 2; i += 1) console.log(`DIAG idle alloc ${i} ${JSON.stringify(await allocLoop(3000))}`);
    for (let i = 0; i < 2; i += 1) console.log(`DIAG idle synthetic scroll ${i} ${JSON.stringify(await synthScroll(4000))}`);
    for (let i = 0; i < 2; i += 1) console.log(`DIAG idle churn ${i} ${JSON.stringify(await churnScroll(4000))}`);
  });
});
