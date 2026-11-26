// Runs after frame.e2e.mjs left dark + German stored and wdio.conf.mjs quit the app. The service
// has started the app again on the same test data directory: the preferences must come back from
// the core, from the very first frame.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { $, $$, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, dataDir, documentState, readState, screenshot, setSidebar, testId } from "./support/app.mjs";

describe("app restart (stage 1)", () => {
  let previous;

  before(async () => {
    previous = readState();
    assert.ok(previous, "frame.e2e.mjs did not finish, so there is nothing to restart from");
    await attach();
  });

  it("is a new app process", async () => {
    const timeOrigin = await browser.execute(() => performance.timeOrigin);
    assert.ok(timeOrigin > previous.timeOrigin, "the page was not loaded again");
    assert.ok(timeOrigin >= previous.finishedAt, "the app was not started again after the first spec");
  });

  it("shows the stored theme and language right away", async () => {
    const state = await documentState();
    assert.equal(state.theme, previous.expect.theme);
    assert.equal(state.lang, previous.expect.language);
    assert.equal(await $("[data-testid=\"app-toolbar\"] h1").getText(), "Posteingang");
    assert.equal(await testId("rail-settings").getAttribute("aria-label"), "Einstellungen");
    assert.deepEqual(await coreCall("get_preferences"), previous.expect);
    assert.deepEqual(await cspViolations(), [], "CSP violations since the page started loading");
    await screenshot("frame-dark-de-after-restart");
  });

  it("keeps the column's views as frame.e2e.mjs chose them (stage 17), then goes back to the default", async () => {
    assert.ok(previous.columnViews, "frame.e2e.mjs chose no views");
    const prefs = await coreCall("get_ui_preferences");
    assert.deepEqual(prefs.columnViews, previous.columnViews);
    const was = await setSidebar(true);
    const columnViews = () =>
      $$('[data-testid="sidebar-views"] [role="treeitem"][data-testid^="sidebar-view-"]').map(async (el) => (await el.getAttribute("data-testid")).replace("sidebar-view-", ""));
    const shown = previous.columnViews.filter((view) => view.shown).map((view) => view.id);
    assert.deepEqual(await columnViews(), shown);
    // The specs after this one expect every view: back to the default (what "Reset to default" stores).
    await coreCall("set_ui_preferences", { preferences: { ...prefs, columnViews: [] } });
    await browser.waitUntil(async () => (await columnViews()).length === shown.length + previous.columnViews.filter((view) => !view.shown).length, {
      timeoutMsg: "the column did not go back to every view",
    });
    await setSidebar(was);
  });

  it("keeps database and logs in the test's own directory", () => {
    const root = dataDir();
    assert.ok(fs.existsSync(path.join(root, "data", "mail.sqlite3")), "database in the test directory");
    const logs = fs.readdirSync(path.join(root, "logs")).filter((f) => f.startsWith("app.") && f.endsWith(".log"));
    assert.equal(logs.length, 1, JSON.stringify(logs));
    const log = fs.readFileSync(path.join(root, "logs", logs[0]), "utf8");
    assert.match(log, /starting/);
    assert.match(log, /database ready/);
    assert.doesNotMatch(log, /\bERROR\b/);
    // The e2e build has an identifier of its own (tauri.e2e.conf.json); had the test directory
    // been ignored, the app would have created this one.
    const fallback = path.join(os.homedir(), "Library", "Application Support", "org.vicemail.desktop.e2e");
    assert.ok(!fs.existsSync(path.join(fallback, "mail.sqlite3")), `${fallback} must not be used`);
  });
});
