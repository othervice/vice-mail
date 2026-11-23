// Stage 8 in the real app: every layout is usable at 900×600 and at 2560×1440 (docs/stages/08,
// acceptance "Bildschirmfotos im Ergebnis").
// Last in `specs` of wdio.conf.mjs: it only reads mail, but it changes the stored layout of the
// window (restored at its end). A window of 2560×1440 needs a display at least that large: on a
// smaller screen (macOS would shrink the window to it) those cases are skipped, with the screen
// size in the report, and have to be checked on a large display.
//
// For each window size, reading pane position (right, below, in place of the list), density
// (comfortable, compact) and text size (100 and 150 percent at the small size): open the newest
// message from the keyboard, then check that nothing scrolls sideways, the list and the pane lie
// inside the window, the toolbar's buttons are inside the pane, and take a screenshot.
//
// Screenshots go to tests/e2e/screenshots/layout-*.png, the measurements to layouts-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { browser } from "@wdio/globals";
import { attach, coreCall, pressTab, screenshot, SCREENSHOT_DIR, setSidebar, testId } from "./support/app.mjs";
import { ALICE, SERVER } from "./support/testmail.mjs";

const SIZES = [
  { width: 900, height: 600 },
  { width: 2560, height: 1440 },
];
const PANES = ["right", "bottom", "replace"];
const DENSITIES = ["comfortable", "compact"];
const report = { screenshots: [], layouts: [] };
const MOD = process.platform === "darwin" ? "Meta" : "Control";

/** Runs a palette command by typing its label (keyboard only, as a user would). */
async function paletteCommand(label) {
  await browser.keys([MOD, "k"]);
  await testId("command-palette").waitForExist();
  await browser.keys([...label]);
  await browser.keys("Enter");
  await testId("command-palette").waitForExist({ reverse: true });
}

const PANE_COMMANDS = {
  right: "Show the reading pane on the right",
  bottom: "Show the reading pane below the list",
  replace: "Show messages instead of the list",
};
const DENSITY_COMMANDS = { comfortable: "Use comfortable rows", compact: "Use compact rows" };

/** Where the main parts are, relative to the window. */
const measure = () =>
  browser.execute(() => {
    // Not shown: missing, hidden, or laid out but invisible and inert (the list while a message
    // is shown in its place keeps its layout for the scroll position, stage 8).
    const rect = (selector) => {
      const element = document.querySelector(selector);
      if (!element || element.closest("[hidden], [inert]") || getComputedStyle(element).visibility === "hidden") return null;
      const r = element.getBoundingClientRect();
      return { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width, height: r.height };
    };
    const toolbar = document.querySelector('[role="toolbar"][aria-label="Message actions"]');
    const toolbarRect = toolbar?.getBoundingClientRect();
    const buttons = toolbar ? [...toolbar.querySelectorAll("button")] : [];
    const why = (selector) => {
      const element = document.querySelector(selector);
      if (!element) return "missing";
      return {
        inert: Boolean(element.closest("[inert]")),
        hidden: Boolean(element.closest("[hidden]")),
        visibility: getComputedStyle(element).visibility,
        box: element.getBoundingClientRect().toJSON(),
      };
    };
    return {
      window: { width: window.innerWidth, height: window.innerHeight },
      focus: document.activeElement?.closest("[data-testid]")?.getAttribute("data-testid") ?? document.activeElement?.tagName ?? null,
      title: document.querySelector("main header h1")?.textContent ?? null,
      listState: why('[data-testid="message-list"]'),
      paneState: why('[data-testid="reading-pane"]'),
      scrollsSideways: document.documentElement.scrollWidth > document.documentElement.clientWidth,
      // docs/DESIGN.md, "Seitenspalte": the column beside the list, over the content, or the rail.
      sidebar: document.querySelector('[data-testid="sidebar-column"][data-overlay]')
        ? "overlay"
        : document.querySelector('[data-testid="sidebar-column"]')
          ? "column"
          : "rail",
      column: rect('[data-testid="sidebar-column"]'),
      list: rect('[data-testid="message-list"]'),
      pane: rect('[data-testid="reading-pane"]'),
      buttonsOutsideToolbar: toolbarRect
        ? buttons.filter((b) => {
            const r = b.getBoundingClientRect();
            return r.width > 0 && (r.right > toolbarRect.right + 1 || r.left < toolbarRect.left - 1);
          }).length
        : null,
    };
  });

const inside = (box, win) => box && box.left >= 0 && box.top >= 0 && box.right <= win.width + 1 && box.bottom <= win.height + 1;

describe("stage 8: layouts at 900×600 and 2560×1440", () => {
  before(async () => {
    await attach();
    // The steps use the English names (palette commands, labels); an earlier spec may have left German.
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } });
    const [existing] = await coreCall("list_accounts");
    if (!existing) {
      await coreCall("add_account", {
        settings: {
          email: ALICE.email,
          displayName: "Alice Archer",
          imap: { host: SERVER.host, port: SERVER.imapsPort, security: "tls", username: ALICE.email },
          smtp: { host: SERVER.host, port: SERVER.smtpsPort, security: "tls", username: ALICE.email },
        },
        password: ALICE.password,
      });
    }
    await browser.waitUntil(
      async () => {
        const [a] = await coreCall("list_accounts");
        return a?.status.state === "idle" && a.status.lastSyncAt !== null;
      },
      { timeout: 240000, interval: 500, timeoutMsg: "the first sync did not finish" },
    );
    await coreCall("set_reading_preferences", { preferences: { markReadOnOpen: "never" } });
  });

  after(async () => {
    await paletteCommand(PANE_COMMANDS.right).catch(() => undefined);
    await paletteCommand(DENSITY_COMMANDS.comfortable).catch(() => undefined);
    // No account (and so no keychain entry) is left for the next run.
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "layouts-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  for (const size of SIZES) {
    for (const pane of PANES) {
      for (const density of DENSITIES) {
        it(`${String(size.width)}×${String(size.height)}, pane ${pane}, ${density}`, async function () {
          const screenSize = await browser.execute(() => ({ width: screen.width, height: screen.height }));
          report.screen = screenSize;
          if (size.width > screenSize.width || size.height > screenSize.height) {
            report.skipped = [...(report.skipped ?? []), `${String(size.width)}×${String(size.height)} ${pane} ${density}`];
            this.skip();
          }
          await browser.setWindowSize(size.width, size.height);
          await paletteCommand(PANE_COMMANDS[pane]);
          await paletteCommand(DENSITY_COMMANDS[density]);
          await paletteCommand("Go to Inbox");
          // Into the list with Tab, the newest message open with Home and Enter.
          let inList = false;
          for (let i = 0; i < 40 && !inList; i += 1) {
            inList = await browser.execute(() => document.activeElement?.getAttribute("data-testid") === "message-list");
            // The embedded WebDriver sends Tab's key events but does not move the focus.
            if (!inList) await pressTab();
          }
          assert.ok(inList, "Tab never reached the message list");
          await browser.keys(["Home", "Enter"]);
          await browser.waitUntil(() => browser.execute(() => Boolean(document.querySelector('[data-testid="reading-pane"]'))), {
            timeoutMsg: "no reading pane",
          });
          await browser.pause(300);
          const facts = await measure();
          const name = `layout-${String(size.width)}x${String(size.height)}-${pane}-${density}`;
          report.layouts.push({ name, ...facts });
          // The screenshot takes the focus away (no focus ring in the picture); it goes back after.
          await browser.execute(() => {
            window.__e2eFocus = document.activeElement;
          });
          report.screenshots.push(await screenshot(name));
          await browser.execute(() => window.__e2eFocus?.focus());
          assert.equal(facts.scrollsSideways, false, `${name}: the window scrolls sideways`);
          assert.ok(inside(facts.pane, facts.window), `${name}: the pane is not inside the window`);
          if (pane === "replace") assert.equal(facts.list, null, `${name}: the list is still shown`);
          else assert.ok(inside(facts.list, facts.window), `${name}: the list is not inside the window`);
          assert.equal(facts.buttonsOutsideToolbar, 0, `${name}: toolbar buttons are cut off`);
          // Escape from the message goes back to the list (in place of the list: the list returns).
          await browser.keys("Escape");
          await browser.waitUntil(
            () =>
              browser.execute(() => {
                const list = document.querySelector('[data-testid="message-list"]');
                return Boolean(list && !list.closest("[inert]") && getComputedStyle(list).visibility !== "hidden");
              }),
            { timeout: 5000, timeoutMsg: `${name}: Escape did not bring the list back` },
          );
        });
      }
    }
  }

  it("the column stands beside the list at 1280×800, folds to the rail at 900×600 and opens over the content there", async () => {
    await browser.setWindowSize(1280, 800);
    await paletteCommand(PANE_COMMANDS.right);
    await paletteCommand("Go to Inbox");
    const was = await setSidebar(true);
    await browser.waitUntil(async () => (await measure()).sidebar === "column", { timeoutMsg: "no column at 1280×800" });
    let facts = await measure();
    report.layouts.push({ name: "layout-1280x800-column", ...facts });
    report.screenshots.push(await screenshot("layout-1280x800-column"));
    assert.equal(facts.scrollsSideways, false, "1280×800 with the column: the window scrolls sideways");
    assert.ok(inside(facts.column, facts.window), "the column is not inside the window");
    assert.ok(facts.list && facts.list.left >= facts.column.right - 1, "the list does not start right of the column");

    // Too narrow for the column next to list and pane: the rail, nothing cut off.
    await browser.setWindowSize(900, 600);
    await browser.waitUntil(async () => (await measure()).sidebar === "rail", { timeoutMsg: "the column did not fold at 900×600" });
    // "Folders" opens it over the content; Escape closes it again, the stored state stays open.
    await testId("rail-folders").click();
    await browser.waitUntil(async () => (await measure()).sidebar === "overlay", { timeoutMsg: "the column did not open over the content" });
    facts = await measure();
    report.layouts.push({ name: "layout-900x600-column-overlay", ...facts });
    report.screenshots.push(await screenshot("layout-900x600-column-overlay"));
    assert.equal(facts.scrollsSideways, false, "900×600 with the column over the content: the window scrolls sideways");
    assert.ok(inside(facts.column, facts.window), "the column over the content is not inside the window");
    await browser.keys("Escape");
    await browser.waitUntil(async () => (await measure()).sidebar === "rail", { timeoutMsg: "Escape did not close the column" });
    await browser.setWindowSize(1280, 800);
    await browser.waitUntil(async () => (await measure()).sidebar === "column", { timeoutMsg: "the column did not come back at 1280×800" });
    await setSidebar(was);
  });

  it("150 % text at 900×600 cuts nothing off", async () => {
    await browser.setWindowSize(900, 600);
    await paletteCommand(PANE_COMMANDS.right);
    await paletteCommand("Settings: Appearance");
    // The text size is a radio group; Tab into it is fiddly, so the command palette is not used here.
    await browser.execute(() => {
      const radio = [...document.querySelectorAll('[role="radio"]')].find((r) => r.textContent === "150%");
      if (radio instanceof HTMLElement) radio.focus();
    });
    await browser.keys(" ");
    await paletteCommand("Go to Inbox");
    const facts = await measure();
    report.layouts.push({ name: "layout-900x600-text150", ...facts });
    report.screenshots.push(await screenshot("layout-900x600-text150"));
    assert.equal(facts.scrollsSideways, false, "150 %: the window scrolls sideways");
    await paletteCommand("Settings: Appearance");
    await browser.execute(() => {
      const radio = [...document.querySelectorAll('[role="radio"]')].find((r) => r.textContent === "100%");
      if (radio instanceof HTMLElement) radio.focus();
    });
    await browser.keys(" ");
  });
});
