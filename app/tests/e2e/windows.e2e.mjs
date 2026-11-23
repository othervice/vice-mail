// Stage 8 (decision 0025) in the real app: a message and a draft in windows of their own, and the
// UI preferences in the core instead of the web view's storage.
// NOT RUN YET (written by the stage 8 UI session, which may not run the desktop tests). It is not
// in `specs` of wdio.conf.mjs; add it after account-sync.e2e.mjs (it needs a synced inbox). It
// only reads mail and writes one local draft, which it discards at the end.
//
// - "Open in new window" from the reading pane opens a second window whose page has the query
//   `?window=message&accountId=…&messageId=…`, shows the message with its toolbar and no rail.
// - A second call for the same message focuses that window instead of opening another.
// - A draft opened with "Open in own window" gets its own window (`?window=composer&draftId=…`),
//   two drafts get two windows; discarding a draft there closes its window (needs the
//   capability `core:window:allow-close` for `composer-*`; without it the window says it is done).
// - A changed shortcut is stored by the core (`get_ui_preferences`), and the web view's storage
//   holds no `vice-mail.ui.shortcuts` key any more.
import assert from "node:assert/strict";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, screenshot, testId } from "./support/app.mjs";

const MOD = process.platform === "darwin" ? "Meta" : "Control";

/** Switches to the window whose page has `window=<kind>`; returns its handle (or null). */
async function switchToWindowOf(kind, timeout = 10000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    for (const handle of await browser.getWindowHandles()) {
      await browser.switchToWindow(handle);
      const search = await browser.execute(() => window.location.search);
      if (new URLSearchParams(search).get("window") === kind) return handle;
    }
    await browser.pause(250);
  }
  return null;
}

describe("stage 8: message and composer windows", () => {
  let main;

  before(async () => {
    await attach();
    main = await browser.getWindowHandle();
  });

  afterEach(async () => {
    await browser.switchToWindow(main);
  });

  it("opens the newest message in a window of its own, once", async () => {
    const list = await $('[role="listbox"][aria-label^="Messages in"]');
    await list.waitForExist({ timeout: 20000 });
    await list.click();
    await browser.keys("Enter");
    const button = await $('[role="toolbar"][aria-label="Message actions"] button[aria-label="Open in new window"]');
    await button.waitForExist();
    await button.click();
    const handle = await switchToWindowOf("message");
    assert.ok(handle, "a message window opened");
    await $('[role="toolbar"][aria-label="Message actions"]').waitForExist({ timeout: 10000 });
    assert.equal(await testId("rail-settings").isExisting(), false, "no rail in the message window");
    await screenshot("window-message");
    const before = (await browser.getWindowHandles()).length;
    await browser.switchToWindow(main);
    await button.click();
    await browser.pause(1000);
    assert.equal((await browser.getWindowHandles()).length, before, "the same message focuses its window");
  });

  it("opens two drafts in two composer windows", async () => {
    for (const subject of ["Window draft A", "Window draft B"]) {
      await browser.switchToWindow(main);
      await browser.keys("c");
      await testId("composer-subject").waitForExist({ timeout: 10000 });
      await testId("composer-subject").click();
      await browser.keys([...subject]);
      await testId("composer-open-window").click();
      await testId("composer").waitForExist({ reverse: true, timeout: 10000 });
    }
    const handles = [];
    for (const handle of await browser.getWindowHandles()) {
      await browser.switchToWindow(handle);
      const search = await browser.execute(() => window.location.search);
      if (new URLSearchParams(search).get("window") === "composer") handles.push(handle);
    }
    assert.equal(handles.length, 2, "one window per draft");
    await screenshot("window-composer");
    for (const handle of handles) {
      await browser.switchToWindow(handle);
      await testId("composer-discard").click();
      await testId("composer-confirm-discard").$("button=Discard").click();
    }
    await browser.pause(1000);
    const left = await browser.getWindowHandles();
    // With `core:window:allow-close` the windows are gone; without, each says it is done.
    if (left.length > 2) {
      await browser.switchToWindow(left[left.length - 1]);
      await testId("window-close-failed").waitForExist({ timeout: 5000 });
    }
  });

  it("keeps a changed shortcut in the core, not in the web view", async () => {
    await browser.switchToWindow(main);
    await browser.keys([MOD, "k"]);
    await testId("command-palette").waitForExist();
    await browser.keys([..."Keyboard"]);
    await browser.keys("Enter");
    await testId("shortcut-row-archive").waitForExist({ timeout: 10000 });
    await testId("shortcut-row-archive").$("button*=Add shortcut").click();
    await browser.keys("y");
    await browser.pause(500);
    const preferences = await coreCall("get_ui_preferences", {});
    const platform = process.platform === "darwin" ? "macos" : "windows";
    assert.ok(
      preferences.shortcuts[platform].some((entry) => entry.command === "archive" && entry.shortcuts.some((s) => s.key === "y")),
      "the core stores the new shortcut",
    );
    const local = await browser.execute(() => window.localStorage.getItem("vice-mail.ui.shortcuts"));
    assert.equal(local, null);
    // Back to the standard assignment for the specs after this one.
    await testId("shortcut-row-archive").$("button*=Reset").click();
  });
});
