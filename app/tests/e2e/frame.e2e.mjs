// Smoke test of the real app (stage 1): the window opens with the empty frame, settings work,
// theme and language switch at once and are stored by the core. Screenshots of the frame in
// light and dark go to tests/e2e/screenshots/. Leaves dark + German stored, which
// restart.e2e.mjs expects after the app was started again.
import assert from "node:assert/strict";
import { $, $$, browser } from "@wdio/globals";
import {
  attach,
  openSidebarParts,
  clearMarks,
  controlBoxes,
  coreCall,
  cspViolations,
  documentState,
  hexToRgb,
  luminance,
  markWindowButtons,
  openSettings,
  openView,
  overlaps,
  pressTab,
  screenshot,
  setPreference,
  setSidebar,
  testId,
  writeState,
} from "./support/app.mjs";

const PRODUCT_NAME = "Vice Mail";
const MOD = process.platform === "darwin" ? "Meta" : "Control";
// The rail by default: the inbox, the column's views (COLUMN_VIEW_IDS), Sent to Folders, then
// activity, the outbox (stage 5) and settings.
const RAIL = [
  // Stage 23 (decision 0085): grouped like the column: the inbox, the views of "All accounts"
  // (Later and the server's flag across folders by default), Sent; then the views under "More".
  "inbox",
  "later",
  "flagged",
  "sent",
  // Stage 20: the sent messages waiting for a reply.
  "waiting",
  "tasks",
  // Stage 16: the reading list and the pinned attachments.
  "readingList",
  "pinnedAttachments",
  // Stage 20: the attachments of every stored message.
  "attachments",
  "pinned",
  // Stage 17: the views of the column's "Views" section, as the user chose them; the contacts
  // are one of them (decision 0050).
  "savedViews",
  "contacts",
  "drafts",
  "archive",
  "trash",
  "folders",
  "activity",
  "outbox",
  "settings",
];

/** Facts collected along the way, printed at the end for the review of the screenshots. */
const report = {};

async function showInbox() {
  await openView("inbox");
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

  it("shows the icon rail, or the accounts-and-folders column in its place, and the list header", async () => {
    // Folded: the narrow rail with every view.
    const was = await setSidebar(false);
    await showInbox();
    // The rail's buttons (the account list and its announcement are no entries).
    const ids = await $$("nav button[data-testid^='rail-']").map((el) => el.getAttribute("data-testid"));
    assert.deepEqual(ids, RAIL.map((id) => `rail-${id}`));
    for (const id of RAIL) assert.ok(await testId(`rail-${id}`).isDisplayed(), `rail-${id} visible`);
    assert.equal(await testId("rail-inbox").getAttribute("aria-label"), "Inbox");

    const header = $("main header h1");
    assert.ok(await header.isDisplayed());
    assert.equal(await header.getText(), "Inbox");

    // Unfolded (docs/DESIGN.md, "Seitenspalte"): the column replaces the rail, nothing twice. Its
    // toggle, the views across folders, then activity, outbox and settings; without an account no
    // folders and no unified inbox, a note instead.
    await setSidebar(true);
    // Stage 23 (decision 0085): "Views" (one account or none: no inbox or Sent there) with Later
    // and Flagged, the rest, Activity and the outbox under "More"; Settings at the foot.
    await openSidebarParts();
    report.column = await $$(
      '[data-testid="sidebar-column"] :is([data-testid^="rail-"], [data-testid^="sidebar-view-"], [data-testid="sidebar-more"], [data-testid="sidebar-activity"], [data-testid="sidebar-outbox"])',
    ).map((el) => el.getAttribute("data-testid"));
    assert.deepEqual(report.column, [
      "rail-folders",
      "sidebar-view-later",
      "sidebar-view-flagged",
      "sidebar-more",
      ...["waiting", "tasks", "readingList", "pinnedAttachments", "attachments", "pinned", "savedViews", "contacts"].map((id) => `sidebar-view-${id}`),
      "sidebar-activity",
      "sidebar-outbox",
      "rail-settings",
    ]);
    assert.equal(await testId("icon-rail").isExisting(), false, "the rail is gone while the column shows");
    assert.ok(await testId("sidebar-no-accounts").isDisplayed());
    assert.equal(await testId("rail-folders").getAttribute("aria-label"), "Hide sidebar");
    await screenshot("frame-column-light");
    await setSidebar(was);
  });

  it("shows the empty state without any invented mail", async () => {
    assert.equal(await $("main h2").getText(), "No account yet");
    const buttons = await $$("main button").map((b) => b.getText());
    assert.ok(buttons.includes("Add account"), JSON.stringify(buttons));
    assert.equal((await $$("main li")).length, 0, "no list rows");
    assert.doesNotMatch(await $("main").getText(), /\d/, "no counters or dates");
    // Without accounts the account switcher in the rail is empty, the column shows no account.
    if (await testId("icon-rail").isExisting()) {
      assert.equal(await browser.execute(() => document.querySelector("nav ul[aria-label]")?.children.length ?? -1), 0);
    } else {
      assert.equal(await browser.execute(() => document.querySelectorAll('[data-testid^="sidebar-account-"]').length), 0);
    }
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
    // Rail or column: the bottom group's settings entry is in both.
    assert.equal(await testId("rail-settings").getAttribute("aria-label"), "Einstellungen");
    assert.equal((await documentState()).lang, "de");
    assert.equal(await browser.execute(() => window.__e2eNoReload === true), true, "no reload");
    await screenshot("settings-general-de-dark");

    await setPreference("language", "en");
    await browser.waitUntil(async () => (await $("main header h1").getText()) === "Settings", { timeout: 2000 });
    assert.equal(await testId("rail-settings").getAttribute("aria-label"), "Settings");
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

  it("edits the column's views (stage 17): hides one, moves one with the keyboard, the core keeps the choice", async () => {
    const was = await setSidebar(true);
    // Stage 23 (decision 0085): every view of the column, the ones under "More" too.
    await openSidebarParts();
    const columnViews = () =>
      $$('[data-testid="sidebar-views"] [role="treeitem"][data-testid^="sidebar-view-"]').map(async (el) => (await el.getAttribute("data-testid")).replace("sidebar-view-", ""));
    // From the palette, by keyboard; the focus starts on the first view.
    await browser.keys([MOD, "k"]);
    await testId("command-palette").waitForExist();
    await browser.keys([..."Edit views"]);
    await browser.keys("Enter");
    await testId("edit-views").waitForExist();
    const focused = () => browser.execute(() => document.activeElement?.getAttribute("data-testid") ?? null);
    await browser.waitUntil(async () => (await focused()) === "edit-views-check-later", { timeoutMsg: "the focus is not on the first view" });
    // A click hides "Later" (the embedded WebDriver performs no default action for Space, so the
    // checkbox is clicked; Vitest covers Space); the column behind the dialog follows at once.
    await testId("edit-views-check-later").click();
    await browser.waitUntil(async () => !(await columnViews()).includes("later"), { timeoutMsg: "Later is still in the column" });
    // The next view: Alt+Down twice moves "Flagged" across the line of "More" (stage 23) and
    // below "Waiting for reply"; the focus stays on it.
    await pressTab();
    assert.equal(await focused(), "edit-views-check-flagged");
    await browser.keys(["Alt", "ArrowDown"]);
    await browser.keys(["Alt", "ArrowDown"]);
    assert.equal(await focused(), "edit-views-check-flagged");
    const shown = ["waiting", "flagged", "tasks", "readingList", "pinnedAttachments", "attachments", "pinned", "savedViews", "contacts"];
    await browser.waitUntil(async () => JSON.stringify(await columnViews()) === JSON.stringify(shown), {
      timeoutMsg: "the column does not show the chosen order",
    });
    await screenshot("edit-views-light");
    const expected = [
      { id: "later", shown: false },
      { id: "more", shown: true },
      ...shown.map((id) => ({ id, shown: true })),
    ];
    await browser.waitUntil(async () => JSON.stringify((await coreCall("get_ui_preferences")).columnViews) === JSON.stringify(expected), {
      timeoutMsg: "the core did not store the chosen views",
    });
    report.columnViews = expected;
    await browser.keys("Escape");
    await testId("edit-views").waitForExist({ reverse: true });

    // Folded: the rail shows the same choice, grouped like the column (nothing before the line
    // of "More" is shown now, so Sent follows the inbox).
    await setSidebar(false);
    const rail = await $$('[data-testid="icon-rail"] [role="toolbar"] button[data-testid^="rail-"]').map((el) => el.getAttribute("data-testid"));
    // Then the bottom group as before (no accounts here, so no account switcher).
    assert.deepEqual(rail, ["inbox", "sent", ...shown, "drafts", "archive", "trash", "folders", "activity", "outbox", "settings"].map((id) => `rail-${id}`));
    await setSidebar(was);
  });

  it("had no CSP violation besides the control while switching views, themes and languages", async () => {
    assert.deepEqual(await cspViolations(), report.cspControl);
  });

  it("leaves dark and German stored for the restart test", async () => {
    await setPreference("theme", "dark");
    await setPreference("language", "de");
    const timeOrigin = await browser.execute(() => performance.timeOrigin);
    // wdio.conf.mjs quits the app after this spec (afterSession); the next one gets a new process.
    // Stage 17: the chosen views must come back after the restart too.
    writeState({ finishedAt: Date.now(), timeOrigin, expect: { theme: "dark", language: "de" }, columnViews: report.columnViews });
  });
});
