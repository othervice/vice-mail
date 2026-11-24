// Stage 18 in the real app, with real mouse events (`e2e_native_mouse_event`: NSEvents handed to
// the WKWebView, decision 0005), docs/stages/18, tasks A and B:
// A. The border between the message list and the reading pane is a handle like the sidebar's.
//    Dragged on the right it sets the list's width, below the list its height; the list's rows
//    switch between one and several lines with its width; a window made smaller keeps the pane's
//    minimum and the stored size; both sizes are stored with the window's layout and are back
//    after the page reloads (the window's layout read again from the core).
// B. Hovering a conversation row shows its quick actions beside the "n replies" toggle, never
//    over it or the selection box: a real click at the toggle's visible centre, with the actions
//    showing, unfolds and folds the conversation. In the wide one-line list and in the narrow
//    list beside the reading pane, and on an unfolded message's row.
//
// On the seed's thread "Planning the 2026 offsite" in Archive, conversations on (turned off again
// at the end, like conversations.e2e.mjs). The stored layout is put back at the end.
// Screenshots go to tests/e2e/screenshots/columns-*.png, the facts to columns-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openView, screenshot, SCREENSHOT_DIR, setConversations, setSidebar, testId } from "./support/app.mjs";
import { ALICE, SERVER } from "./support/testmail.mjs";

const report = { screenshots: [], steps: {} };
let preferencesBefore = null;
let layoutBefore = null;
let sidebarBefore = null;
let conversationId = null;
const MOD = process.platform === "darwin" ? "Meta" : "Control";

/** Runs a palette command by typing its label, as layouts.e2e.mjs does. */
async function paletteCommand(label) {
  await browser.keys([MOD, "k"]);
  await testId("command-palette").waitForExist();
  await browser.keys([...label]);
  await browser.keys("Enter");
  await testId("command-palette").waitForExist({ reverse: true });
}

/**
 * One real mouse event at a point of the viewport (CSS pixels). The test machine's window is
 * often not the key window (a locked screen): `e2e_native_mouse_event` sends a move so that
 * WebKit still updates `:hover` then.
 */
async function mouse(kind, point) {
  const viewportHeight = await browser.execute(() => innerHeight);
  const result = await coreCall("e2e_native_mouse_event", { kind, x: point.x, y: point.y, viewportHeight });
  report.mouse = [...(report.mouse ?? []).slice(-20), result];
  return result;
}

/** The page's transitions done, so computed colours are the end state (see `screenshot`). */
const settle = () =>
  browser.execute(() => {
    for (const animation of document.getAnimations()) animation.finish();
  });

/** A real click (down and up) at a point of the viewport. */
async function click(point) {
  const viewportHeight = await browser.execute(() => innerHeight);
  return coreCall("e2e_native_mouse", { x: point.x, y: point.y, viewportHeight });
}

/** Presses on `from`, drags in steps to `to` and lets go there, like a hand on the mouse. */
async function drag(from, to, steps = 8) {
  await mouse("move", from);
  await mouse("down", from);
  for (let i = 1; i <= steps; i += 1) {
    await mouse("drag", { x: from.x + ((to.x - from.x) * i) / steps, y: from.y + ((to.y - from.y) * i) / steps });
    await browser.pause(20);
  }
  await mouse("up", to);
  await browser.pause(100);
}

/** The handle between list and pane, the list it sizes, the pane, and the rows' layout. */
const split = () =>
  browser.execute(() => {
    const box = (el) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height };
    };
    const handle = document.querySelector('[data-testid="list-resize"]');
    const list = handle ? document.getElementById(handle.getAttribute("aria-controls") ?? "") : null;
    const firstRow = document.querySelector('[data-testid="message-list"] [data-testid="message-row"]');
    return {
      scale: Number.parseFloat(getComputedStyle(document.documentElement).fontSize) / 16,
      window: { width: innerWidth, height: innerHeight },
      handle: box(handle),
      orientation: handle?.getAttribute("aria-orientation") ?? null,
      label: handle?.getAttribute("aria-label") ?? null,
      value: handle ? Number(handle.getAttribute("aria-valuenow")) : null,
      max: handle ? Number(handle.getAttribute("aria-valuemax")) : null,
      hovered: handle?.matches(":hover") ?? false,
      line: handle ? getComputedStyle(handle, "::after").backgroundColor : null,
      list: box(list),
      pane: box(handle?.nextElementSibling ?? null),
      container: box(handle?.parentElement ?? null),
      // Narrow rows lay their lines out in a column (MessageRowContent).
      narrowRows: firstRow ? firstRow.querySelector(".flex-col") !== null : null,
      scrollsSideways: document.documentElement.scrollWidth > document.documentElement.clientWidth,
    };
  });

const centre = (box) => ({ x: box.left + box.width / 2, y: box.top + box.height / 2 });
const storedLayout = () => coreCall("get_window_layout");

/** Opens the first row of the list on the right (or below): a synthetic click selects it. */
async function openFirstRow() {
  await (await browser.$('[data-testid="message-list"] [role="treeitem"][aria-level="1"]')).click();
  await testId("list-resize").waitForExist({ timeout: 10000 });
}

/** Where a conversation row's parts are, which of them the pointer is over, what shows. */
const rowParts = (id) =>
  browser.execute((rowId) => {
    const row = document.getElementById(rowId);
    const box = (el) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height };
    };
    const toggle = row?.querySelector('[data-testid="row-replies"]');
    const actions = row?.querySelector('[data-testid="row-actions"]');
    const check = row?.querySelector("[data-row-check]");
    const t = box(toggle);
    const at = t ? document.elementFromPoint(t.left + t.width / 2, t.top + t.height / 2) : null;
    return {
      hovered: row?.matches(":hover") ?? false,
      expanded: row?.getAttribute("aria-expanded") ?? null,
      narrow: Boolean(row?.querySelector(".flex-col")),
      row: box(row),
      toggle: t,
      check: box(check),
      actions: box(actions),
      actionsShown: actions ? getComputedStyle(actions).display !== "none" && getComputedStyle(actions).visibility !== "hidden" : false,
      targets: actions ? actions.querySelectorAll('[data-testid^="quick-"]').length : 0,
      metaShown: (() => {
        const time = row?.querySelector('[data-testid="row-meta"] time');
        return time ? time.getClientRects().length > 0 : null;
      })(),
      // What a click at the toggle's centre reaches: the toggle itself.
      atToggleCentre: at ? (at.closest("[data-row-toggle]") === toggle ? "toggle" : (at.closest("[data-testid]")?.getAttribute("data-testid") ?? at.tagName)) : null,
      children: document.querySelectorAll('[data-testid="child-row"]').length,
    };
  }, id);

const overlaps = (a, b) => Boolean(a && b && a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom);

async function scrollRowIntoView(id) {
  await browser.execute((rowId) => {
    document.getElementById(rowId)?.scrollIntoView({ block: "center" });
  }, id);
  await browser.pause(150);
}

/**
 * Hovers the conversation row with the real pointer (over its subject), checks that the quick
 * actions show beside the toggle and that nothing clickable is under them, then clicks the
 * toggle's visible centre to unfold and again to fold. Returns the facts.
 */
async function hoverAndToggle(where) {
  await scrollRowIntoView(conversationId);
  let parts = await rowParts(conversationId);
  assert.equal(parts.expanded, "false", `${where}: the conversation is unfolded already`);
  // Onto the row, left of the toggle: the row is hovered and shows its actions.
  await mouse("move", { x: parts.toggle.left - 12, y: parts.toggle.top + parts.toggle.height / 2 });
  await browser.waitUntil(async () => (await rowParts(conversationId)).actionsShown, { timeout: 5000, timeoutMsg: `${where}: no quick actions on hover` });
  parts = await rowParts(conversationId);
  const facts = { where, hoveredRow: parts.hovered, narrow: parts.narrow, toggle: parts.toggle, actions: parts.actions, targets: parts.targets };
  assert.ok(parts.hovered, `${where}: the row is not hovered`);
  assert.ok(parts.targets >= 4, `${where}: only ${String(parts.targets)} quick actions`);
  assert.equal(parts.metaShown, false, `${where}: the time shows beside the quick actions`);
  assert.ok(!overlaps(parts.actions, parts.toggle), `${where}: the quick actions cover the toggle ${JSON.stringify(parts)}`);
  assert.ok(!overlaps(parts.actions, parts.check), `${where}: the quick actions cover the selection box`);
  assert.ok(parts.actions.right <= parts.row.right + 0.5 && parts.actions.top >= parts.row.top - 0.5 && parts.actions.bottom <= parts.row.bottom + 0.5, `${where}: the quick actions leave the row`);
  assert.equal(parts.atToggleCentre, "toggle", `${where}: at the toggle's centre is ${String(parts.atToggleCentre)}`);
  report.screenshots.push(await screenshot(`columns-hover-${where}`));

  // A real click at the toggle's visible centre while the actions show: unfolds.
  const point = centre(parts.toggle);
  facts.click = await click(point);
  await browser.waitUntil(async () => (await rowParts(conversationId)).expanded === "true", { timeout: 10000, timeoutMsg: `${where}: the click did not unfold` });
  await browser.waitUntil(async () => (await rowParts(conversationId)).children > 0, { timeout: 15000, timeoutMsg: `${where}: no messages below` });
  parts = await rowParts(conversationId);
  assert.ok(parts.actionsShown, `${where}: the quick actions went away under the pointer`);
  assert.equal(parts.atToggleCentre, "toggle", `${where}: unfolded, at the toggle's centre is ${String(parts.atToggleCentre)}`);
  report.screenshots.push(await screenshot(`columns-hover-${where}-unfolded`));

  // An unfolded message's row: its actions in its line, clear of its selection box.
  const child = await browser.execute(() => {
    const row = document.querySelectorAll('[data-testid="child-row"]')[1];
    if (!row) return null;
    const r = row.getBoundingClientRect();
    return { id: row.id, x: r.left + r.width / 2, y: r.top + r.height / 2 };
  });
  assert.ok(child, `${where}: no second message row`);
  await mouse("move", child);
  const childParts = await browser.execute((id) => {
    const row = document.getElementById(id);
    const box = (el) => {
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height };
    };
    const actions = row.querySelector('[data-testid="row-actions"]');
    return {
      hovered: row.matches(":hover"),
      shown: getComputedStyle(actions).display !== "none",
      row: box(row),
      actions: box(actions),
      check: box(row.querySelector("[data-row-check]")),
    };
  }, child.id);
  facts.child = childParts;
  assert.ok(childParts.hovered && childParts.shown, `${where}: no quick actions on the message row`);
  assert.ok(!overlaps(childParts.actions, childParts.check), `${where}: the message row's actions cover its selection box`);
  assert.ok(childParts.actions.top >= childParts.row.top - 0.5 && childParts.actions.bottom <= childParts.row.bottom + 0.5, `${where}: the message row's actions leave the row`);

  // Back on the conversation row, the same click folds it again.
  await mouse("move", { x: point.x - 30, y: point.y });
  parts = await rowParts(conversationId);
  assert.ok(parts.actionsShown, `${where}: no quick actions on the way back`);
  assert.equal(parts.atToggleCentre, "toggle");
  await click(centre(parts.toggle));
  await browser.waitUntil(async () => (await rowParts(conversationId)).expanded === "false", { timeout: 10000, timeoutMsg: `${where}: the click did not fold` });
  await browser.waitUntil(async () => (await rowParts(conversationId)).children === 0, { timeout: 10000, timeoutMsg: `${where}: the messages stayed` });
  facts.folded = true;
  // Away from the list, so no row stays hovered.
  await mouse("move", { x: 2, y: 2 });
  return facts;
}

/** The offsite thread's row in Archive (its id changes when the page reloads). */
async function findConversation() {
  await browser.waitUntil(
    async () => {
      conversationId = await browser.execute(() => {
        const rows = [...document.querySelectorAll('[data-testid="message-list"] [role="treeitem"][aria-level="1"]')].filter((row) =>
          row.querySelector('[data-testid="row-replies"]'),
        );
        const offsite = rows.find((row) => row.querySelector('[data-testid="row-name"]')?.textContent.includes("Planning the 2026 offsite"));
        return (offsite ?? rows[0])?.id ?? null;
      });
      return conversationId !== null;
    },
    { timeout: 20000, timeoutMsg: "no conversation row with replies in Archive" },
  );
}

describe("stage 18: the border between list and pane moves; unfolding beside the quick actions", () => {
  before(async () => {
    await attach();
    preferencesBefore = await coreCall("get_preferences");
    await coreCall("set_preferences", { preferences: { ...preferencesBefore, language: "en", theme: "light" } });
    await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.lang)) === "en", {
      timeoutMsg: "the app did not switch to English",
    });
    layoutBefore = await storedLayout();
    await browser.setWindowSize(1280, 800);
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
    await setConversations(true);
    sidebarBefore = await setSidebar(true);
    await paletteCommand("Show the reading pane on the right");
    await openView("archive");
    await browser.waitUntil(async () => (await testId("message-list").getAttribute("role")) === "tree", {
      timeout: 20000,
      timeoutMsg: "the list of conversations is no tree",
    });
    await findConversation();
  });

  after(async () => {
    await mouse("move", { x: 2, y: 2 }).catch(() => undefined);
    if (layoutBefore) await coreCall("set_window_layout", { layout: layoutBefore }).catch(() => undefined);
    if (sidebarBefore !== null) await setSidebar(sidebarBefore).catch(() => undefined);
    if (preferencesBefore) await coreCall("set_preferences", { preferences: preferencesBefore }).catch(() => undefined);
    await setConversations(false).catch(() => undefined);
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "columns-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("A: drags the list's width on the right; rows follow the width; a smaller window keeps the pane", async () => {
    await openFirstRow();
    const start = await split();
    report.steps.rightStart = start;
    assert.equal(start.orientation, "vertical");
    assert.equal(start.label, "List width");
    assert.equal(start.scale, 1, "the test expects 100 % text");
    assert.ok(Math.abs(start.list.width - start.value) <= 1, `the list is ${String(start.list.width)} px, the handle says ${String(start.value)}`);
    assert.equal(start.narrowRows, true, "the list beside the pane has narrow rows");
    // The handle straddles the hairline: half over the list, half over the pane, 8 px.
    assert.ok(Math.abs(start.handle.width - 8) <= 0.5 && Math.abs(start.handle.left + 4 - start.list.right) <= 1, JSON.stringify(start.handle));

    // Hovered, it shows the accent line (as the sidebar's does).
    await mouse("move", centre(start.handle));
    await settle();
    const hovered = await split();
    assert.ok(hovered.hovered, "the handle is not hovered under the pointer");
    assert.notEqual(hovered.line, "rgba(0, 0, 0, 0)", "no line on the hovered handle");
    report.screenshots.push(await screenshot("columns-handle-right-hover"));

    // A real drag 120 px to the right: the list grows by as much.
    await drag(centre(start.handle), { x: centre(start.handle).x + 120, y: centre(start.handle).y });
    const wider = await split();
    report.steps.rightDragged = wider;
    assert.ok(Math.abs(wider.list.width - (start.list.width + 120)) <= 2, `dragged 120 px: the list went from ${String(start.list.width)} to ${String(wider.list.width)}`);
    assert.ok(Math.abs(wider.value - (start.value + 120)) <= 2, `the handle says ${String(wider.value)}`);
    assert.ok(Math.abs(wider.pane.left - wider.list.right) <= 1, "the pane does not start at the list's border");
    await browser.waitUntil(async () => (await storedLayout()).listWidth === wider.value, { timeout: 5000, timeoutMsg: "the width was not stored" });

    // Without the sidebar there is room for a list over 720 px: its rows get one line, and
    // narrow again when it is dragged back below.
    await setSidebar(false);
    const room = await split();
    await drag(centre(room.handle), { x: room.list.left + 800, y: centre(room.handle).y });
    const wide = await split();
    report.steps.rightWide = wide;
    assert.ok(wide.list.width > 720, `the list did not get wider than 720 px: ${String(wide.list.width)}`);
    await browser.waitUntil(async () => (await split()).narrowRows === false, { timeout: 5000, timeoutMsg: "rows kept several lines in a wide list" });
    assert.ok(wide.pane.width >= 360 - 1, `the pane got narrower than its minimum: ${String(wide.pane.width)}`);
    report.screenshots.push(await screenshot("columns-right-wide-list"));
    await drag(centre(wide.handle), { x: wide.list.left + 500, y: centre(wide.handle).y });
    await browser.waitUntil(async () => (await split()).narrowRows === true, { timeout: 5000, timeoutMsg: "rows kept one line in a narrow list" });
    const back = await split();
    assert.ok(Math.abs(back.list.width - 500) <= 2, `the list is ${String(back.list.width)} px, not 500`);
    await browser.waitUntil(async () => (await storedLayout()).listWidth === back.value, { timeout: 5000, timeoutMsg: "the width was not stored" });
    await setSidebar(true);
    assert.ok(Math.abs((await split()).list.width - 500) <= 2, "the sidebar changed the list's width");

    // A smaller window (the sidebar folds to the rail by itself there): the pane keeps its
    // 360 px, the list gives way, the stored width stays.
    try {
      await browser.setWindowSize(800, 600);
      await browser.waitUntil(
        async () => {
          const now = await split();
          return now.window.width <= 800 && now.container.left < 100;
        },
        { timeout: 10000, timeoutMsg: "the window did not get smaller or the sidebar did not fold" },
      );
      const small = await split();
      report.steps.rightSmall = small;
      assert.ok(small.pane.width >= 359, `the pane got ${String(small.pane.width)} px in a small window`);
      assert.ok(small.list.width >= 319, `the list got ${String(small.list.width)} px`);
      assert.equal(small.scrollsSideways, false, "the small window scrolls sideways");
      assert.ok(small.value <= small.max && small.value < 500, `the handle says ${String(small.value)} of at most ${String(small.max)}`);
      assert.equal((await storedLayout()).listWidth, 500, "the window's size changed the stored width");
      report.screenshots.push(await screenshot("columns-right-small-window"));
    } finally {
      await browser.setWindowSize(1280, 800);
    }
    await browser.waitUntil(
      async () => {
        const now = await split();
        return now.container.left >= 200 && Math.abs(now.list.width - 500) <= 2;
      },
      { timeout: 10000, timeoutMsg: "the sidebar or the list's 500 px did not come back" },
    );
  });

  it("A: drags the list's height when the pane is below", async () => {
    await paletteCommand("Show the reading pane below the list");
    await openFirstRow();
    const start = await split();
    report.steps.bottomStart = start;
    assert.equal(start.orientation, "horizontal");
    assert.equal(start.label, "List height");
    assert.ok(Math.abs(start.list.height - start.value) <= 1, `the list is ${String(start.list.height)} px high, the handle says ${String(start.value)}`);
    await mouse("move", centre(start.handle));
    report.screenshots.push(await screenshot("columns-handle-bottom-hover"));
    await drag(centre(start.handle), { x: centre(start.handle).x, y: centre(start.handle).y + 100 });
    const taller = await split();
    report.steps.bottomDragged = taller;
    assert.ok(Math.abs(taller.list.height - (start.list.height + 100)) <= 2, `dragged 100 px: the list went from ${String(start.list.height)} to ${String(taller.list.height)}`);
    assert.ok(taller.pane.height >= 239, `the pane got ${String(taller.pane.height)} px`);
    await browser.waitUntil(async () => (await storedLayout()).listHeight === taller.value, { timeout: 5000, timeoutMsg: "the height was not stored" });
    // Far down: the pane keeps 240 px.
    await drag(centre(taller.handle), { x: centre(taller.handle).x, y: taller.container.bottom - 20 });
    const lowest = await split();
    assert.ok(lowest.pane.height >= 239, `dragged down to the bottom, the pane got ${String(lowest.pane.height)} px`);
    await drag(centre(lowest.handle), { x: centre(lowest.handle).x, y: lowest.list.top + taller.list.height });
    const settled = await split();
    assert.ok(Math.abs(settled.list.height - taller.list.height) <= 2, `the list is ${String(settled.list.height)} px high`);
    await browser.waitUntil(async () => (await storedLayout()).listHeight === settled.value, { timeout: 5000, timeoutMsg: "the height was not stored" });
    report.steps.stored = await storedLayout();
  });

  it("A: both sizes are back after the page reloads the window's layout", async () => {
    const stored = await storedLayout();
    await browser.execute(() => {
      window.location.reload();
    });
    await browser.pause(2000);
    await attach();
    await openView("archive");
    await testId("message-list").waitForExist({ timeout: 20000 });
    await openFirstRow();
    const below = await split();
    report.steps.reloadedBottom = below;
    assert.equal(below.orientation, "horizontal");
    assert.equal(below.value, stored.listHeight);
    assert.ok(Math.abs(below.list.height - stored.listHeight) <= 1, `after the reload the list is ${String(below.list.height)} px high`);
    await paletteCommand("Show the reading pane on the right");
    await openFirstRow();
    const right = await split();
    report.steps.reloadedRight = right;
    assert.equal(right.value, stored.listWidth);
    assert.ok(Math.abs(right.list.width - stored.listWidth) <= 1, `after the reload the list is ${String(right.list.width)} px wide`);
    assert.deepEqual(await cspViolations(), [], "CSP violations");
  });

  it("B: a real click on the replies toggle unfolds and folds while the quick actions show", async () => {
    await findConversation();
    // Beside the reading pane: narrow rows, the actions on the first line, the toggle below.
    await (await browser.$('[data-testid="message-list"] [role="treeitem"][aria-level="1"]:not([aria-expanded])')).click();
    await testId("list-resize").waitForExist({ timeout: 10000 });
    report.steps.narrow = await hoverAndToggle("narrow");
    assert.equal(report.steps.narrow.narrow, true, "the list beside the pane is not narrow");

    // A list dragged wider than 720 px (no sidebar): one line a row, the actions after the
    // subject's part of the line.
    const narrowWidth = (await split()).list.width;
    await setSidebar(false);
    const room = await split();
    await drag(centre(room.handle), { x: room.list.left + 820, y: centre(room.handle).y });
    await browser.waitUntil(async () => (await split()).narrowRows === false, { timeout: 5000, timeoutMsg: "rows kept several lines in a wide list" });
    try {
      report.steps.wide = await hoverAndToggle("wide");
      assert.equal(report.steps.wide.narrow, false, "the wide list's row is not one line");
      // Compact rows: the same in both widths.
      await paletteCommand("Use compact rows");
      report.steps.wideCompact = await hoverAndToggle("wide-compact");
    } finally {
      const wide = await split();
      await drag(centre(wide.handle), { x: wide.list.left + narrowWidth, y: centre(wide.handle).y });
      await setSidebar(true);
    }
    try {
      await browser.waitUntil(async () => (await split()).narrowRows === true, { timeout: 5000, timeoutMsg: "rows kept one line in a narrow list" });
      report.steps.narrowCompact = await hoverAndToggle("narrow-compact");
    } finally {
      await paletteCommand("Use comfortable rows");
    }
    assert.deepEqual(await cspViolations(), [], "CSP violations");
  });
});
