// Stage 15 (decision 0044) in the real app: the guided first start of a new installation. Runs
// first, on the fresh data directory of the run (every later spec skips nothing: the first start
// is over by then). Walks the five steps without adding an account (account-sync.e2e.mjs needs
// the data directory without one), checks the focus on each heading, that Escape keeps what was
// typed, the window buttons and the layout at the default size and at 900×600, and takes
// screenshots of every step in light and dark. Leaves: first start completed, automatic update
// checks off (answered), theme "system", English.
import assert from "node:assert/strict";
import { browser } from "@wdio/globals";
import { attach, controlBoxes, coreCall, cspViolations, documentState, overlaps, screenshot, testId } from "./support/app.mjs";

const SMALL = { width: 900, height: 600 };
const report = { screenshots: [], layout: [] };

/** Light or dark through the core, as the first start's own picker stores it. */
async function setTheme(theme) {
  await coreCall("set_preferences", { preferences: { theme, language: "en" } });
  await browser.waitUntil(async () => (await documentState()).theme === theme, { timeoutMsg: `theme ${theme} not applied` });
}

/** Screenshots of the current step in light and dark; ends in light. */
async function shootBothThemes(name) {
  for (const theme of ["light", "dark"]) {
    await setTheme(theme);
    report.screenshots.push(await screenshot(`first-start-${name}-${theme}`));
  }
  await setTheme("light");
}

const step = () => testId("first-start").getAttribute("data-step");

/** The focused element: the step's heading after every move. */
const focused = () =>
  browser.execute(() => ({ tag: document.activeElement?.tagName ?? null, text: document.activeElement?.textContent ?? null }));

async function expectHeadingFocused(text) {
  await browser.waitUntil(async () => {
    const now = await focused();
    return now.tag === "H1" && now.text === text;
  }, { timeoutMsg: `the focus is not on the heading "${text}" (${JSON.stringify(await focused())})` });
}

async function goOn(testid, nextStep, heading) {
  await testId(testid).click();
  await browser.waitUntil(async () => (await step()) === nextStep, { timeoutMsg: `step ${nextStep} did not show` });
  await expectHeadingFocused(heading);
}

/** Where the card, its body and the footer buttons are, and whether anything scrolls sideways. */
function layoutFacts() {
  return browser.execute(() => {
    const box = (el) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { x: r.left, y: r.top, width: r.width, height: r.height };
    };
    const buttons = [...document.querySelectorAll('[data-testid="first-start"] footer button')].map(box);
    return {
      window: { x: 0, y: 0, width: innerWidth, height: innerHeight },
      card: box(document.querySelector('[data-testid="first-start-body"]')?.parentElement ?? null),
      buttons,
      skip: box(document.querySelector('[data-testid="first-start-skip"]')),
      scrollsSideways: document.documentElement.scrollWidth > innerWidth || document.body.scrollWidth > innerWidth,
    };
  });
}

const inside = (inner, outer) =>
  inner && inner.x >= outer.x - 0.5 && inner.y >= outer.y - 0.5 && inner.x + inner.width <= outer.x + outer.width + 0.5 && inner.y + inner.height <= outer.y + outer.height + 0.5;

async function checkLayout(name) {
  const facts = await layoutFacts();
  report.layout.push({ name, ...facts });
  assert.equal(facts.scrollsSideways, false, `${name}: the window scrolls sideways`);
  assert.ok(inside(facts.card, facts.window), `${name}: the card is not inside the window`);
  for (const button of facts.buttons) assert.ok(inside(button, facts.window), `${name}: a footer button is cut off`);
  if (facts.skip) assert.ok(inside(facts.skip, facts.window), `${name}: "Skip setup" is cut off`);
}

describe("guided first start (stage 15, decision 0044)", () => {
  let initialSize;

  before(async () => {
    await attach({ firstStart: "keep" });
    initialSize = await browser.getWindowSize();
    // The steps are checked by their English texts; the system language may be German.
    await setTheme("light");
  });

  after(async () => {
    if (initialSize) await browser.setWindowSize(initialSize.width, initialSize.height);
    await coreCall("set_preferences", { preferences: { theme: "system", language: "en" } }).catch(() => undefined);
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("starts with the welcome step instead of the frame, the focus on its heading, without CSP violations", async () => {
    assert.equal(await step(), "welcome");
    assert.equal(await testId("rail-settings").isExisting(), false, "no frame behind the first start");
    assert.deepEqual(await coreCall("get_first_start"), { show: true, outcome: null });
    await expectHeadingFocused("Welcome");
    const text = await testId("first-start-welcome").getText();
    for (const promise of ["only read at first", "No cloud service in between", "No telemetry", "system keychain"]) {
      assert.ok(text.includes(promise), `promise "${promise}" missing`);
    }
    assert.deepEqual(await cspViolations(), [], "CSP violations since the page started loading");
  });

  it("keeps the macOS window buttons clear of its controls", async () => {
    const controls = await coreCall("e2e_window_controls");
    const boxes = await controlBoxes();
    const clashes = [];
    for (const button of controls.buttons) {
      for (const box of boxes) if (overlaps(button, box)) clashes.push(`${button.name} over ${box.label}`);
    }
    assert.deepEqual(clashes, []);
  });

  it("welcome: fits the default window and 900×600, light and dark", async () => {
    await checkLayout("welcome-default");
    await shootBothThemes("welcome");
    await browser.setWindowSize(SMALL.width, SMALL.height);
    await browser.pause(200);
    await checkLayout("welcome-900x600");
    await shootBothThemes("welcome-900x600");
    await browser.setWindowSize(initialSize.width, initialSize.height);
  });

  it("account: the setup form in the step; Escape keeps what was typed", async () => {
    await goOn("first-start-next", "account", "Add your first account");
    await shootBothThemes("account");
    const email = testId("setup-email");
    await email.setValue("someone@example.org");
    await browser.keys("Escape");
    await browser.pause(200);
    assert.equal(await step(), "account", "Escape left the step");
    assert.equal(await email.getValue(), "someone@example.org", "Escape lost what was typed");
    await browser.setWindowSize(SMALL.width, SMALL.height);
    await browser.pause(200);
    await checkLayout("account-900x600");
    await shootBothThemes("account-900x600");
    await browser.setWindowSize(initialSize.width, initialSize.height);
    await email.clearValue();
  });

  it("appearance: theme and accent apply at once and the core stores them", async () => {
    await goOn("first-start-later", "appearance", "Choose your look");
    await testId("first-start-theme-dark").click();
    await browser.waitUntil(async () => (await documentState()).theme === "dark", { timeoutMsg: "dark not applied" });
    assert.equal((await coreCall("get_preferences")).theme, "dark");
    await testId("accent-orange").click();
    await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.accent)) === "orange", {
      timeoutMsg: "orange not applied",
    });
    assert.equal((await coreCall("get_ui_preferences")).accentTheme, "orange");
    // Back to the default (decision 0053), so the screenshots show it.
    await testId("accent-blue").click();
    await browser.waitUntil(async () => (await coreCall("get_ui_preferences")).accentTheme === "blue", {
      timeoutMsg: "blue not stored again",
    });
    await shootBothThemes("appearance");
    await checkLayout("appearance-default");
  });

  it("extras: sync needs an account, AI is only explained, the update switch is off", async () => {
    await goOn("first-start-next", "extras", "Optional extras");
    const list = await testId("first-start-extras-list").getText();
    assert.ok(list.includes("Needs an account"), "the sync explains that it needs an account");
    assert.ok(list.includes("AI over MCP"), "AI says where to turn it on");
    const toggle = testId("first-start-auto-check");
    assert.equal(await toggle.getAttribute("aria-checked"), "false", "update checks are off by default");
    await shootBothThemes("extras");
    await browser.setWindowSize(SMALL.width, SMALL.height);
    await browser.pause(200);
    await checkLayout("extras-900x600");
    await shootBothThemes("extras-900x600");
    await browser.setWindowSize(initialSize.width, initialSize.height);
  });

  it("done: says where to add an account, and ending it shows the frame for good", async () => {
    await goOn("first-start-next", "done", "You’re all set");
    // Leaving the extras step answered the update question: still off.
    assert.equal((await coreCall("get_update_info")).autoCheck, false);
    assert.ok((await testId("first-start-no-account").getText()).includes("Settings → Accounts"));
    assert.equal(await testId("first-start-skip").isExisting(), false, "no skip on the last step");
    await shootBothThemes("done");
    await testId("first-start-finish").click();
    await testId("rail-settings").waitForExist({ timeout: 20000 });
    assert.equal(await testId("first-start").isExisting(), false);
    assert.deepEqual(await coreCall("get_first_start"), { show: false, outcome: "completed" });
    report.screenshots.push(await screenshot("first-start-after-light"));
    assert.deepEqual(await cspViolations(), [], "CSP violations during the first start");
  });
});
