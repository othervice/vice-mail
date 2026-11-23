// Smoke test of the real app (stage 1): the window opens with the empty frame, settings work,
// theme and language switch at once and are stored by the core. Screenshots of the frame in
// light and dark go to tests/e2e/screenshots/. Leaves dark + German stored, which
// restart.e2e.mjs expects after the app was started again.
import assert from "node:assert/strict";
import { $, $$, browser } from "@wdio/globals";
import {
  attach,
  clearMarks,
  controlBoxes,
  coreCall,
  cspViolations,
  documentState,
  hexToRgb,
  luminance,
  markWindowButtons,
  openSettings,
  overlaps,
  screenshot,
  setPreference,
  testId,
  writeState,
} from "./support/app.mjs";

const PRODUCT_NAME = "Vice Mail";
const RAIL = ["inbox", "later", "tasks", "pinned", "sent", "drafts", "archive", "trash", "folders", "savedViews", "activity", "settings"];

/** Facts collected along the way, printed at the end for the review of the screenshots. */
const report = {};

async function showInbox() {
  await testId("rail-inbox").click();
  await browser.waitUntil(async () => (await testId("rail-inbox").getAttribute("aria-current")) === "page");
}

describe("app frame (stage 1)", () => {
  before(async () => {
    await attach();
  });

  after(() => {
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("opens one window with the app, without CSP violations", async () => {
    assert.equal((await browser.getWindowHandles()).length, 1);
    assert.match(await browser.getUrl(), /^tauri:\/\/localhost/);
    assert.equal(await browser.getTitle(), PRODUCT_NAME);

    const state = await documentState();
    report.viewport = state.viewport;
    report.visibilityState = await browser.execute(() => document.visibilityState);
    assert.equal(state.platform, "macos");
    // The stylesheet loaded under the strict CSP: the body has the token colour.
    assert.equal(state.bodyBackground, hexToRgb(state.canvasToken));

    assert.deepEqual(await cspViolations(), [], "CSP violations since the page started loading");
  });

  it("detects a CSP violation when there is one (positive control)", async () => {
    // An inline script is what the app's `script-src 'self'` forbids. (Inline styles no longer
    // are: the mail frame inherits the app's CSP and needs `style-src 'unsafe-inline'`, 0005.)
    const ran = await browser.execute(async () => {
      window.__e2eInlineScriptRan = false;
      const probe = document.createElement("script");
      probe.textContent = "window.__e2eInlineScriptRan = true;";
      document.body.append(probe);
      probe.remove();
      await new Promise((resolve) => setTimeout(resolve, 50));
      return window.__e2eInlineScriptRan;
    });
    assert.equal(ran, false, "the CSP must block inline scripts");
    await browser.waitUntil(async () => (await cspViolations()).length > 0, {
      timeoutMsg: "the recorder saw no violation for the blocked inline script",
    });
    const violations = await cspViolations();
    report.cspControl = violations;
    assert.equal(violations.length, 1, JSON.stringify(violations));
    assert.match(violations[0].directive, /^script-src/);
    assert.equal(violations[0].blocked, "inline");
  });

  it("starts in English after choosing it (the system language may be German)", async () => {
    await setPreference("language", "en");
    assert.equal((await documentState()).lang, "en");
  });

  it("shows the icon rail and the list header", async () => {
    await showInbox();
    const ids = await $$("nav [data-testid^='rail-']").map((el) => el.getAttribute("data-testid"));
    assert.deepEqual(ids, RAIL.map((id) => `rail-${id}`));
    for (const id of RAIL) assert.ok(await testId(`rail-${id}`).isDisplayed(), `rail-${id} visible`);
    assert.equal(await testId("rail-inbox").getAttribute("aria-label"), "Inbox");

    const header = $("main header h1");
    assert.ok(await header.isDisplayed());
    assert.equal(await header.getText(), "Inbox");
  });

  it("shows the empty state without any invented mail", async () => {
    assert.equal(await $("main h2").getText(), "No account yet");
    const buttons = await $$("main button").map((b) => b.getText());
    assert.ok(buttons.includes("Add account"), JSON.stringify(buttons));
    assert.equal((await $$("main li")).length, 0, "no list rows");
    assert.doesNotMatch(await $("main").getText(), /\d/, "no counters or dates");
    // Without accounts the account switcher in the rail is empty.
    assert.equal(await browser.execute(() => document.querySelector("nav ul[aria-label]")?.children.length ?? -1), 0);
  });

  it("keeps the macOS window buttons clear of the frame's controls", async () => {
    const controls = await coreCall("e2e_window_controls");
    const state = await documentState();
    report.windowControls = controls;
    report.titlebarInsetToken = state.titlebarInset;
    assert.equal(controls.buttons.length, 3);
    const boxes = await controlBoxes();
    const clashes = [];
    for (const button of controls.buttons) {
      for (const box of boxes) if (overlaps(button, box)) clashes.push(`${button.name} over ${box.label}`);
    }
    report.windowButtonClashes = clashes;
    assert.deepEqual(clashes, []);
    // The buttons sit inside the title-bar band the frame keeps free.
    const inset = Number.parseFloat(state.titlebarInset);
    for (const b of controls.buttons) assert.ok(b.y + b.height <= inset, `${b.name} below the ${inset}px inset`);
  });

  it("opens the settings with all five sections", async () => {
    await testId("rail-settings").click();
    assert.equal(await $("main header h1").getText(), "Settings");
    for (const id of ["general", "accounts", "reading", "appearance", "about"]) {
      assert.ok(await testId(`settings-tab-${id}`).isDisplayed(), `${id} tab`);
    }
  });

  it("switches to light and dark at once and stores the choice", async () => {
    await setPreference("theme", "light");
    const light = await documentState();
    assert.equal(light.theme, "light");
    assert.equal(light.bodyBackground, hexToRgb(light.canvasToken));
    await screenshot("settings-appearance-light");
    await showInbox();
    await screenshot("frame-light");
    const controls = await coreCall("e2e_window_controls");
    await markWindowButtons(controls.buttons);
    await screenshot("frame-light-window-buttons");
    await clearMarks();

    await setPreference("theme", "dark");
    const dark = await documentState();
    assert.equal(dark.theme, "dark");
    assert.equal(dark.bodyBackground, hexToRgb(dark.canvasToken));
    await screenshot("settings-appearance-dark");
    await showInbox();
    await screenshot("frame-dark");
    await markWindowButtons(controls.buttons);
    await screenshot("frame-dark-window-buttons");
    await clearMarks();

    report.bodyBackground = { light: light.bodyBackground, dark: dark.bodyBackground };
    assert.notEqual(light.bodyBackground, dark.bodyBackground);
    assert.ok(luminance(light.bodyBackground) - luminance(dark.bodyBackground) > 0.5, "light is clearly lighter");
  });

  it("switches the language at once, without reloading", async () => {
    await browser.execute(() => {
      window.__e2eNoReload = true;
    });
    await setPreference("language", "de");
    // Right after the click, not after a reload: the marker survives and the texts are German.
    await browser.waitUntil(async () => (await $("main header h1").getText()) === "Einstellungen", {
      timeout: 2000,
      timeoutMsg: "the settings title did not turn German",
    });
    assert.equal(await testId("settings-tab-general").getText(), "Allgemein");
    assert.equal(await testId("rail-inbox").getAttribute("aria-label"), "Posteingang");
    assert.equal((await documentState()).lang, "de");
    assert.equal(await browser.execute(() => window.__e2eNoReload === true), true, "no reload");
    await screenshot("settings-general-de-dark");

    await setPreference("language", "en");
    await browser.waitUntil(async () => (await $("main header h1").getText()) === "Settings", { timeout: 2000 });
    assert.equal(await testId("rail-inbox").getAttribute("aria-label"), "Inbox");
    assert.equal(await browser.execute(() => window.__e2eNoReload === true), true, "no reload");
  });

  it("shows version and platform on the About page", async () => {
    const info = await coreCall("get_app_info");
    assert.equal(info.platform, "macos");
    await openSettings("about");
    const rows = await browser.execute(() => {
      const out = {};
      for (const dt of document.querySelectorAll("main dl dt")) out[dt.textContent.trim()] = dt.nextElementSibling?.textContent.trim();
      return out;
    });
    report.about = rows;
    assert.equal(rows.Version, info.version);
    assert.match(rows.Version, /^\d+\.\d+\.\d+/);
    assert.equal(rows.Platform, "macOS");
    await screenshot("settings-about-dark");
  });

  it("had no CSP violation besides the control while switching views, themes and languages", async () => {
    assert.deepEqual(await cspViolations(), report.cspControl);
  });

  it("leaves dark and German stored for the restart test", async () => {
    await setPreference("theme", "dark");
    await setPreference("language", "de");
    const timeOrigin = await browser.execute(() => performance.timeOrigin);
    // wdio.conf.mjs quits the app after this spec (afterSession); the next one gets a new process.
    writeState({ finishedAt: Date.now(), timeOrigin, expect: { theme: "dark", language: "de" } });
  });
});
