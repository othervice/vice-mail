// Measures how smoothly the message list scrolls (account-sync.e2e.mjs, stage 2).
//
// The test scrolls the list itself, frame by frame, and records:
// - frame intervals: time between two scroll steps. In a visible window the steps run in
//   requestAnimationFrame, so these are the real frame times. In a hidden window (one started
//   from SSH or tmux, or behind a locked screen) WebKit runs no animation frames at all; then a
//   task loop on a MessageChannel (which WebKit does not throttle) takes a step every 16.7 ms, and
//   the intervals show how far other work on the main thread delayed the next step.
// - main-thread blocking: a MessageChannel loop runs between all other tasks; a gap between two
//   of its turns is the length of whatever ran in between (WebKit has no `longtask` entries).
// - the cost of each step itself: setting scrollTop and, in a hidden window, the scroll event
//   with everything the list does synchronously in it.
// - blank frames: right after each step, whether the rows in the DOM cover the visible part of
//   the list, i.e. whether the frame painted next would show empty space.
// - how often the list grew during the measurement (the sync stored messages, the list reloaded).
//
// What a hidden window cannot show: paint and compositing times. Layout is included, because
// the blank check forces it on every step.
import { browser } from "@wdio/globals";

/**
 * Scrolls down at `pxPerFrame` for 65 % of `durationMs`, then back up at twice the speed.
 * Returns the raw numbers and a summary; asserts nothing.
 */
export async function measureScroll({ durationMs = 8000, pxPerFrame = 80 } = {}) {
  await browser.setTimeout({ script: durationMs + 30000 });
  return browser.executeAsync(
    (opts, done) => {
      const FRAME_MS = 1000 / 60;
      const list = document.querySelector('[data-testid="message-list"]');
      if (!list) {
        done({ error: "no message list on screen" });
        return;
      }
      const syncText = () => document.querySelector('[data-testid="sync-status"]')?.textContent.trim() ?? null;
      const setSize = () => Number(list.querySelector('[role="option"]')?.getAttribute("aria-setsize") ?? 0);

      // Uncovered pixels of the visible list area: no row (message, section or "loading more")
      // in the DOM there. The padding above the first and below the last row is not a gap.
      const PADDING = 8;
      const uncovered = () => {
        const view = list.getBoundingClientRect();
        const top = view.top + Math.max(0, PADDING - list.scrollTop);
        const bottom = view.bottom - Math.max(0, PADDING - (list.scrollHeight - list.scrollTop - list.clientHeight));
        const rects = [...list.querySelectorAll("[data-index]")]
          .map((el) => el.getBoundingClientRect())
          .filter((r) => r.bottom > top && r.top < bottom)
          .sort((a, b) => a.top - b.top);
        let reached = top;
        let gap = 0;
        for (const r of rects) {
          if (r.top > reached + 0.5) gap += r.top - reached;
          reached = Math.max(reached, r.bottom);
        }
        if (reached < bottom - 0.5) gap += bottom - reached;
        return gap;
      };

      const result = {
        mode: null,
        visibilityState: document.visibilityState,
        listSizes: [],
        frames: [],
        stepCosts: [],
        gaps: [],
        blankFrames: [],
        bottomHits: 0,
        maxScrollTop: 0,
        before: { sync: syncText(), setSize: setSize() },
      };

      // Main-thread monitor and, in a hidden window, the step clock.
      const channel = new MessageChannel();
      let running = true;
      let lastTickEnd = performance.now();
      const onTick = [];
      channel.port1.onmessage = () => {
        const now = performance.now();
        const gap = now - lastTickEnd;
        if (gap > 4) result.gaps.push(Math.round(gap * 10) / 10);
        for (const fn of onTick) fn(now);
        lastTickEnd = performance.now();
        if (running) channel.port2.postMessage(0);
      };
      channel.port2.postMessage(0);

      let started = 0;
      let lastStep = 0;
      const step = (now) => {
        if (lastStep) result.frames.push(Math.round((now - lastStep) * 10) / 10);
        lastStep = now;
        const t0 = performance.now();
        const max = list.scrollHeight - list.clientHeight;
        const down = now - started < opts.durationMs * 0.65;
        if (down && list.scrollTop >= max - 1) result.bottomHits += 1;
        const delta = down ? opts.pxPerFrame : -2 * opts.pxPerFrame;
        list.scrollTop = Math.max(0, Math.min(max, list.scrollTop + delta));
        result.maxScrollTop = Math.max(result.maxScrollTop, list.scrollTop);
        const size = setSize();
        if (result.listSizes.at(-1) !== size) result.listSizes.push(size);
        const gap = uncovered();
        if (gap > 0.5) result.blankFrames.push({ at: Math.round(now - started), px: Math.round(gap), scrollTop: list.scrollTop });
        // A hidden window runs no rendering steps, so it fires no scroll events; the list
        // listens for them, so the test sends the one a visible window sends once per frame.
        if (result.mode === "tasks") list.dispatchEvent(new Event("scroll"));
        result.stepCosts.push(Math.round((performance.now() - t0) * 10) / 10);
      };

      const finish = () => {
        running = false;
        const sorted = (values) => [...values].sort((a, b) => a - b);
        const pick = (values, q) => (values.length ? sorted(values)[Math.min(values.length - 1, Math.floor(q * values.length))] : null);
        const frames = result.frames;
        result.after = { sync: syncText(), setSize: setSize() };
        result.summary = {
          mode: result.mode,
          durationMs: opts.durationMs,
          pxPerFrame: opts.pxPerFrame,
          frames: frames.length,
          frameMedianMs: pick(frames, 0.5),
          frameP95Ms: pick(frames, 0.95),
          frameMaxMs: frames.length ? Math.max(...frames) : null,
          framesOver17Ms: frames.filter((f) => f > 17).length,
          framesOver33Ms: frames.filter((f) => f > 33).length,
          framesOver50Ms: frames.filter((f) => f > 50).length,
          stepCostMedianMs: pick(result.stepCosts, 0.5),
          stepCostMaxMs: result.stepCosts.length ? Math.max(...result.stepCosts) : null,
          longestBlockingMs: result.gaps.length ? Math.max(...result.gaps) : 0,
          blockingsOver17Ms: result.gaps.filter((g) => g > 17).length,
          blockingsOver50Ms: result.gaps.filter((g) => g > 50).length,
          blankFrames: result.blankFrames.length,
          bottomHits: result.bottomHits,
          rowsScrolled: Math.round(result.maxScrollTop / 44),
          listGrewTimes: result.listSizes.length - 1,
          listSizes: result.listSizes,
          before: result.before,
          after: result.after,
        };
        done(result);
      };

      // A visible window answers requestAnimationFrame within a frame or two; give it 250 ms.
      let rafSeen = false;
      requestAnimationFrame(() => {
        rafSeen = true;
      });
      const probeUntil = performance.now() + 250;
      const decide = (now) => {
        if (result.mode !== null || (!rafSeen && now < probeUntil)) return;
        result.mode = rafSeen ? "raf" : "tasks";
        started = now;
        if (result.mode === "raf") {
          const frame = (t) => {
            if (t - started >= opts.durationMs) {
              finish();
              return;
            }
            step(t);
            requestAnimationFrame(frame);
          };
          requestAnimationFrame(frame);
        } else {
          let next = now;
          onTick.push((t) => {
            if (!running || t < next) return;
            if (t - started >= opts.durationMs) {
              finish();
              return;
            }
            step(t);
            // After a delay the missed frames are dropped, not made up for in a burst.
            next += FRAME_MS;
            if (next <= t) next = t + FRAME_MS;
          });
        }
      };
      onTick.push(decide);
    },
    { durationMs, pxPerFrame },
  );
}
