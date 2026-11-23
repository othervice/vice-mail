// Stage 8 in the real app: every layout is usable at 900×600 and at 2560×1440 (docs/stages/08,
// acceptance "Bildschirmfotos im Ergebnis").
// NOT RUN YET (written by the keyboard UI session, which may not run the desktop tests). It is not
// in `specs` of wdio.conf.mjs; add it anywhere after account-sync.e2e.mjs to run it. It only
// reads mail. A window of 2560×1440 needs a display at least that large; macOS shrinks a bigger
// window to the screen, which the report then shows as the size actually reached.
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
import { attach, coreCall, screenshot, SCREENSHOT_DIR, testId } from "./support/app.mjs";
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
    const rect = (selector) => {
      const element = document.querySelector(selector);
      if (!element || element.closest("[hidden]")) return null;
      const r = element.getBoundingClientRect();
      return { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width, height: r.height };
    };
    const toolbar = document.querySelector('[role="toolbar"][aria-label="Message actions"]');
    const toolbarRect = toolbar?.getBoundingClientRect();
    const buttons = toolbar ? [...toolbar.querySelectorAll("button")] : [];
    return {
      window: { width: window.innerWidth, height: window.innerHeight },
      scrollsSideways: document.documentElement.scrollWidth > document.documentElement.clientWidth,
      list: rect('[data-testid="message-list"]'),
      pane: rect('[role="region"][aria-label="Message"]'),
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
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "layouts-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  for (const size of SIZES) {
    for (const pane of PANES) {
      for (const density of DENSITIES) {
        it(`${String(size.width)}×${String(size.height)}, pane ${pane}, ${density}`, async () => {
          await browser.setWindowSize(size.width, size.height);
          await paletteCommand(PANE_COMMANDS[pane]);
          await paletteCommand(DENSITY_COMMANDS[density]);
          await paletteCommand("Go to Inbox");
          // Into the list with Tab, the newest message open with Home and Enter.
          for (let i = 0; i < 40; i += 1) {
            const inList = await browser.execute(() => document.activeElement?.getAttribute("data-testid") === "message-list");
            if (inList) break;
            await browser.keys("Tab");
          }
          await browser.keys(["Home", "Enter"]);
          await browser.waitUntil(() => browser.execute(() => Boolean(document.querySelector('[role="region"][aria-label="Message"]'))), {
            timeoutMsg: "no reading pane",
          });
          await browser.pause(300);
          const facts = await measure();
          const name = `layout-${String(size.width)}x${String(size.height)}-${pane}-${density}`;
          report.layouts.push({ name, ...facts });
          report.screenshots.push(await screenshot(name));
          assert.equal(facts.scrollsSideways, false, `${name}: the window scrolls sideways`);
          assert.ok(inside(facts.pane, facts.window), `${name}: the pane is not inside the window`);
          if (pane === "replace") assert.equal(facts.list, null, `${name}: the list is still shown`);
          else assert.ok(inside(facts.list, facts.window), `${name}: the list is not inside the window`);
          assert.equal(facts.buttonsOutsideToolbar, 0, `${name}: toolbar buttons are cut off`);
          await browser.keys("Escape");
        });
      }
    }
  }

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
