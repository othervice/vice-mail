// Helpers for driving the real app through the embedded WebDriver (decision 0005).
//
// WebDriver clicks are synthetic (`el.click()` plus `el.focus()`), which is enough for the app's
// buttons, Radix tabs (they switch on focus) and radio groups.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { $, browser } from "@wdio/globals";
import { DATA_DIR_ENV } from "./lifecycle.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));

/** PNGs of the last run; not checked in (.gitignore). */
export const SCREENSHOT_DIR = path.join(here, "..", "screenshots");

export function dataDir() {
  const dir = process.env[DATA_DIR_ENV];
  if (!dir) throw new Error(`${DATA_DIR_ENV} is not set; run the tests through wdio.conf.mjs`);
  return dir;
}

/** Handover between the two specs (the second one runs after an app restart). */
const statePath = () => path.join(dataDir(), "e2e-state.json");

export function writeState(state) {
  fs.writeFileSync(statePath(), JSON.stringify(state, null, 2));
}

export function readState() {
  return fs.existsSync(statePath()) ? JSON.parse(fs.readFileSync(statePath(), "utf8")) : null;
}

/** Attaches to the app window and waits until the frame is rendered. */
export async function attach() {
  // @wdio/tauri-service re-focuses the "active" window before getTitle, $ and clicks through
  // tauri-plugin-wdio, which this app does not include; every such command would wait 5 s.
  // An explicit switchToWindow turns that off (decision 0005).
  await browser.switchToWindow(await browser.getWindowHandle());
  await $('[data-testid="rail-settings"]').waitForExist({ timeout: 20000 });
}

/**
 * Calls a core command the way the UI does, without the generated bindings: `{ ok }` or
 * `{ failure }`. Not `{ error }`: webdriverio reads a returned object with an `error` key as a
 * WebDriver error response and throws its own error with the message instead.
 */
export function invoke(command, args = {}) {
  return browser.execute(
    async (name, payload) => {
      try {
        return { ok: await window.__TAURI_INTERNALS__.invoke(name, payload) };
      } catch (e) {
        return { failure: typeof e === "string" ? e : JSON.stringify(e) };
      }
    },
    command,
    args,
  );
}

export async function coreCall(command, args) {
  const result = await invoke(command, args);
  if ("failure" in result) throw new Error(`${command} failed: ${result.failure}`);
  return result.ok;
}

export const testId = (id) => $(`[data-testid="${id}"]`);

export async function openSettings(section) {
  await testId("rail-settings").click();
  const tab = testId(`settings-tab-${section}`);
  await tab.click();
  await browser.waitUntil(async () => (await tab.getAttribute("data-state")) === "active", {
    timeoutMsg: `settings section ${section} did not open`,
  });
}

/**
 * Picks `value` in the open settings section (theme or language) and waits until the core has
 * stored it, so the choice is not only shown but kept.
 */
export async function setPreference(kind, value) {
  await openSettings(kind === "theme" ? "appearance" : "general");
  const radio = $(`[role="radio"][value="${value}"]`);
  await radio.click();
  await browser.waitUntil(async () => (await radio.getAttribute("aria-checked")) === "true", {
    timeoutMsg: `${kind} ${value} is not selected`,
  });
  await browser.waitUntil(async () => (await coreCall("get_preferences"))[kind] === value, {
    timeoutMsg: `the core did not store ${kind} = ${value}`,
  });
}

export function documentState() {
  return browser.execute(() => ({
    theme: document.documentElement.dataset.theme ?? null,
    platform: document.documentElement.dataset.platform ?? null,
    lang: document.documentElement.lang,
    title: document.title,
    bodyBackground: getComputedStyle(document.body).backgroundColor,
    canvasToken: getComputedStyle(document.documentElement).getPropertyValue("--ui-canvas").trim(),
    titlebarInset: getComputedStyle(document.documentElement).getPropertyValue("--ui-titlebar-inset").trim(),
    viewport: { width: innerWidth, height: innerHeight, scale: devicePixelRatio },
  }));
}

/** Relative luminance of `rgb(r, g, b)` (WCAG). */
export function luminance(rgb) {
  const parts = rgb.match(/\d+(\.\d+)?/g);
  if (!parts || parts.length < 3) throw new Error(`not an rgb() colour: ${rgb}`);
  const [r, g, b] = parts.slice(0, 3).map((v) => {
    const c = Number(v) / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** `#rrggbb` as the `rgb(r, g, b)` string getComputedStyle returns. */
export function hexToRgb(hex) {
  const n = Number.parseInt(hex.replace("#", ""), 16);
  return `rgb(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255})`;
}

/**
 * Every CSP violation since the page started loading, recorded by the e2e build's init script
 * (`CSP_RECORDER` in src-tauri/src/e2e.rs). Throws if the recorder is missing.
 */
export async function cspViolations() {
  const violations = await browser.execute(() => window.__e2eCspViolations ?? null);
  if (violations === null) throw new Error("the CSP recorder of the e2e build is missing");
  return violations;
}

/**
 * Takes a screenshot of the web view. WebDriver renders only the web content: the window
 * buttons macOS draws on top are not in the picture (see `markWindowButtons`).
 */
export async function screenshot(name) {
  fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
  await browser.execute(() => {
    // No focus ring or focus tooltip from the last synthetic click in the picture.
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    // A window started from a terminal session (SSH, tmux) is never key and, with the display
    // asleep, "hidden" for WebKit: no animation frames, so CSS transitions stay on their first
    // frame and the picture would show the previous colours. A visible window finishes them
    // within --ui-duration-fast; here they are finished explicitly.
    for (const animation of document.getAnimations()) animation.finish();
  });
  await browser.pause(100);
  const file = path.join(SCREENSHOT_DIR, `${name}.png`);
  await browser.saveScreenshot(file);
  return file;
}

/**
 * Draws the native window buttons where macOS puts them (from `e2e_window_controls`) as
 * outlined circles into the page, for a review screenshot. `clearMarks` removes them again.
 * Styles go through the CSSOM, which the app's CSP allows.
 */
export function markWindowButtons(buttons) {
  return browser.execute((rects) => {
    const colours = { close: "#ff5f57", minimize: "#febc2e", zoom: "#28c840" };
    for (const r of rects) {
      const mark = document.createElement("div");
      mark.dataset.e2eMark = r.name;
      Object.assign(mark.style, {
        position: "fixed",
        left: `${r.x}px`,
        top: `${r.y}px`,
        width: `${r.width}px`,
        height: `${r.height}px`,
        borderRadius: "50%",
        background: colours[r.name] ?? "#888888",
        outline: "1px dashed #ff00ff",
        outlineOffset: "1px",
        zIndex: "2147483647",
        pointerEvents: "none",
      });
      document.body.append(mark);
    }
  }, buttons);
}

export const clearMarks = () =>
  browser.execute(() => {
    for (const mark of document.querySelectorAll("[data-e2e-mark]")) mark.remove();
  });

/** Visible boxes of the frame's controls and headings, to check the window buttons against. */
export function controlBoxes() {
  return browser.execute(() =>
    [...document.querySelectorAll("button, h1, h2, [role=tab], [role=radio]")]
      .map((el) => {
        const r = el.getBoundingClientRect();
        const label = el.getAttribute("aria-label") ?? el.textContent?.trim() ?? el.tagName;
        return { label: `${el.tagName.toLowerCase()} "${label}"`, x: r.left, y: r.top, width: r.width, height: r.height };
      })
      .filter((b) => b.width > 0 && b.height > 0),
  );
}

export function overlaps(a, b) {
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
}
