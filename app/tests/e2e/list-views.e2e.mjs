// Stage 19, task A in the real app (decisions 0061, 0062), on alice's inbox of the seed (about
// 10,000 messages), conversations off (rows are messages), the reading pane below the list (the
// list is wide):
// 1. Sorting: the view menu sorts by sender, subject and size, both directions; the rows are the
//    core's order of that view; the order is stored for the view and back to newest first it is
//    gone again. The time from the click to the rows drawn in the new order is recorded.
// 2. Table: chosen in the view menu; column headers with aria-sort, a click on "Subject" sorts by
//    it and a second click turns it around; a real drag of the sender column's handle widens it
//    (header and rows line up), the arrow keys move it by 16 px, the width is stored; the view
//    menu hides the size column. The keyboard walks the rows as in the list (listbox; with
//    conversations on a tree of treeitems). axe finds nothing serious, light and dark.
// 3. Quick filter: Unread, Flagged, With attachment, a tag (put on three messages for this spec,
//    removed again), the text field (search language), combined; the bar's count is the core's;
//    Reset; Cmd+Shift+K puts the focus into the field, the switches are one toolbar.
//
// Screenshots go to tests/e2e/screenshots/list-views-*.png, the facts to list-views-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openView, screenshot, SCREENSHOT_DIR, setConversations, setSidebar, testId } from "./support/app.mjs";
import { ALICE, SERVER } from "./support/testmail.mjs";

const require = createRequire(import.meta.url);
const AXE_SOURCE = fs.readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const MOD = process.platform === "darwin" ? "Meta" : "Control";

const report = { screenshots: [], steps: {}, timings: {}, notes: [] };
let account = null;
let inbox = null;
let preferencesBefore = null;
let layoutBefore = null;
let tableBefore = null;
let sidebarBefore = null;
let tagged = [];

const requestId = () => crypto.randomUUID();
const scope = () => ({ kind: "mailbox", mailboxId: inbox.id });
const viewKey = () => `mailbox:${String(inbox.id)}`;
const NO_FILTER = { unread: false, flagged: false, attachment: false, contacts: false, tag: null, text: "", monthFirst: false };

/** Message ids of the rows drawn, top down (the option ids end in the message id). */
const drawnIds = () =>
  browser.execute(() =>
    [...document.querySelectorAll('[data-testid="message-list"] [role="option"], [data-testid="message-list"] [role="treeitem"]')]
      .sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top)
      .map((el) => Number(el.id.split("-").at(-1))),
  );

/** The first `n` ids of the core's own page of this view. */
async function coreIds(order, filter = NO_FILTER, n = 20) {
  const page = await coreCall("list_view_messages", { request: { scope: scope(), order, filter }, cursor: null, limit: n });
  return page.items.map((item) => item.id);
}

/** Waits until the first drawn rows are `ids`. */
async function waitRows(ids, what) {
  await browser.waitUntil(
    async () => {
      const drawn = await drawnIds();
      return ids.length > 0 && ids.slice(0, Math.min(8, ids.length)).every((id, i) => drawn[i] === id);
    },
    { timeout: 20000, timeoutMsg: `${what}: the rows are not in the core's order` },
  );
}

/**
 * Clicks `selector` inside the page and measures until the first drawn row is `firstId` (DOM
 * time, polled every 5 ms: the test window may be hidden, where WebKit runs no animation frames;
 * perf.e2e.mjs measures painted frames with the page kept visible).
 */
async function timedClick(selector, firstId) {
  await browser.setTimeout({ script: 30000 });
  return browser.executeAsync(
    (sel, id, done) => {
      const target = document.querySelector(sel);
      if (!target) {
        done({ failure: `no ${sel}` });
        return;
      }
      const started = performance.now();
      target.click();
      const timer = setInterval(() => {
        const first = [...document.querySelectorAll('[data-testid="message-list"] [role="option"], [data-testid="message-list"] [role="treeitem"]')].sort(
          (a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top,
        )[0];
        if (first && Number(first.id.split("-").at(-1)) === id) {
          clearInterval(timer);
          done({ ms: Math.round(performance.now() - started) });
        } else if (performance.now() - started > 20000) {
          clearInterval(timer);
          done({ failure: "timeout" });
        }
      }, 5);
    },
    selector,
    firstId,
  );
}

/** Runs a palette command by typing its label, as layouts.e2e.mjs does. */
async function paletteCommand(label) {
  await browser.keys([MOD, "k"]);
  await testId("command-palette").waitForExist();
  await browser.keys([...label]);
  await browser.keys("Enter");
  await testId("command-palette").waitForExist({ reverse: true });
}

async function openViewMenu() {
  if ((await testId("list-view-menu").getAttribute("aria-expanded")) !== "true") await testId("list-view-menu").click();
  await testId("list-view-menu-menu").waitForExist({ timeout: 5000 });
}

async function closeMenus() {
  await browser.keys("Escape");
  await testId("list-view-menu-menu").waitForExist({ reverse: true, timeout: 5000 });
}

/** One real mouse event (`e2e_native_mouse_event`), as columns.e2e.mjs. */
async function mouse(kind, point) {
  const viewportHeight = await browser.execute(() => innerHeight);
  return coreCall("e2e_native_mouse_event", { kind, x: point.x, y: point.y, viewportHeight });
}

async function drag(from, to, steps = 8) {
  await mouse("move", from);
  await mouse("down", from);
  for (let i = 1; i <= steps; i += 1) {
    await mouse("drag", { x: from.x + ((to.x - from.x) * i) / steps, y: from.y + ((to.y - from.y) * i) / steps });
    await browser.pause(20);
  }
  await mouse("up", to);
  await browser.pause(150);
}

async function setTheme(theme) {
  await coreCall("set_preferences", { preferences: { ...preferencesBefore, language: "en", theme } });
  await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme, {
    timeoutMsg: `the ${theme} theme was not applied`,
  });
}

async function seriousAxe() {
  const present = await browser.execute(() => typeof window.axe);
  if (present !== "object") await browser.execute(`${AXE_SOURCE}\n;return typeof window.axe;`);
  await browser.setTimeout({ script: 60000 });
  const result = await browser.executeAsync((done) => {
    for (const animation of document.getAnimations()) animation.finish();
    window.axe
      .run(document, { resultTypes: ["violations"], iframes: false })
      .then((r) => {
        done({ ok: r.violations.map((v) => ({ id: v.id, impact: v.impact, nodes: v.nodes.slice(0, 5).map((n) => n.target.join(" ")) })) });
      })
      .catch((e) => {
        done({ failure: String(e) });
      });
  });
  if ("failure" in result) throw new Error(`axe.run failed: ${result.failure}`);
  return result.ok.filter((v) => v.impact === "serious" || v.impact === "critical");
}

/** Where the header's and the first row's cells are, by column. */
const alignment = () =>
  browser.execute(() => {
    const box = (el) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { left: Math.round(r.left * 10) / 10, width: Math.round(r.width * 10) / 10 };
    };
    const header = Object.fromEntries(
      [...document.querySelectorAll('[data-testid="table-header"] [role="columnheader"]')].map((h) => [h.dataset.testid?.replace("column-", ""), box(h)]),
    );
    const row = document.querySelector('[data-testid="message-list"] [data-testid="table-row"]');
    const cells = row ? Object.fromEntries([...row.querySelectorAll('[data-testid^="cell-"]')].map((c) => [c.dataset.testid.replace("cell-", ""), box(c)])) : {};
    return { header, cells };
  });

describe("stage 19, A: sorting, the table and the quick filter", () => {
  before(async () => {
    await attach();
    preferencesBefore = await coreCall("get_preferences");
    await coreCall("set_preferences", { preferences: { ...preferencesBefore, language: "en", theme: "light" } });
    await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.lang)) === "en", {
      timeoutMsg: "the app did not switch to English",
    });
    layoutBefore = await coreCall("get_window_layout");
    tableBefore = await coreCall("get_list_table");
    await browser.setWindowSize(1280, 860);
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
    [account] = await coreCall("list_accounts");
    inbox = (await coreCall("list_mailboxes", { accountId: account.id })).find((m) => m.role === "inbox");
    assert.ok(inbox, "alice has no inbox");
    report.steps.inbox = { total: inbox.total, unread: inbox.unread };
    await coreCall("set_reading_preferences", { preferences: { markReadOnOpen: "never" } });
    await coreCall("set_list_order", { view: viewKey(), order: null });
    await coreCall("set_list_table", { settings: { ...tableBefore, presentation: "rows" } });
    await setConversations(false);
    sidebarBefore = await setSidebar(true);
    // The pane below the list: the list keeps the window's width (rows and the table are wide).
    await paletteCommand("Show the reading pane below the list");
    // The update question of a fresh data directory covers the list in the screenshots.
    if (await testId("update-consent-no").isExisting()) {
      await testId("update-consent-no").click();
      await testId("update-consent").waitForExist({ reverse: true });
    }
    await openView("inbox");
    await testId("message-list").waitForExist({ timeout: 20000 });
    await testId("quick-filter").waitForExist({ timeout: 10000 });
  });

  after(async () => {
    await mouse("move", { x: 2, y: 2 }).catch(() => undefined);
    if (tagged.length > 0) {
      await coreCall("apply_action", {
        action: { type: "removeKeyword", keyword: "Projekt" },
        messages: tagged.map((id) => ({ mailboxId: inbox.id, messageId: id })),
        requestId: requestId(),
      }).catch((e) => {
        report.notes.push(`tag cleanup failed: ${String(e)}`);
      });
    }
    if (account) await coreCall("set_writes_enabled", { accountId: account.id, enabled: false }).catch(() => undefined);
    if (inbox) await coreCall("set_list_order", { view: viewKey(), order: null }).catch(() => undefined);
    if (tableBefore) await coreCall("set_list_table", { settings: tableBefore }).catch(() => undefined);
    if (layoutBefore) await coreCall("set_window_layout", { layout: layoutBefore }).catch(() => undefined);
    if (sidebarBefore !== null) await setSidebar(sidebarBefore).catch(() => undefined);
    if (preferencesBefore) await coreCall("set_preferences", { preferences: preferencesBefore }).catch(() => undefined);
    report.csp = await cspViolations().catch(() => null);
    // No account (and so no keychain entry) is left for the next spec, as columns.e2e.mjs.
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => report.notes.push(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "list-views-report.json"), JSON.stringify(report, null, 2));
  });

  it("1: sorts by sender, subject and size in both directions, stored for the view", async () => {
    // Newest first, the inbox is its own sorted list (priority block, bundles, date sections).
    await browser.waitUntil(async () => (await browser.execute(() => document.querySelectorAll('[data-testid="list-section"]').length)) > 0, {
      timeout: 20000,
      timeoutMsg: "the inbox shows no date sections",
    });
    await openViewMenu();
    report.screenshots.push(await screenshot("list-views-sort-menu"));
    const expectations = [
      { key: "sender", ascending: true, item: "sort-key-sender" },
      { key: "sender", ascending: false, item: "sort-descending" },
      { key: "subject", ascending: true, item: "sort-key-subject" },
      { key: "size", ascending: false, item: "sort-key-size" },
      { key: "size", ascending: true, item: "sort-ascending" },
    ];
    for (const { key, ascending, item } of expectations) {
      const order = { key, ascending };
      const ids = await coreIds(order);
      await openViewMenu();
      const timed = await timedClick(`[data-testid="${item}"]`, ids[0]);
      assert.ok(!("failure" in timed), `${key} ${String(ascending)}: ${JSON.stringify(timed)}`);
      await waitRows(ids, `${key} ${ascending ? "ascending" : "descending"}`);
      report.timings[`sort ${key} ${ascending ? "asc" : "desc"}`] = timed.ms;
      const stored = await coreCall("get_list_orders");
      assert.deepEqual(stored.find((o) => o.view === viewKey())?.order, order, `stored order for ${key}`);
      // No date headings in another order.
      assert.equal(await browser.execute(() => document.querySelectorAll('[data-testid="list-section"]').length), 0, "date sections while sorted");
    }
    const name = await testId("list-view-menu").getAttribute("aria-label");
    assert.equal(name, "Sorted by Size, Smallest first");
    // The keyboard walks the sorted rows: ↓ moves the cursor to the next row of this order.
    const ids = await coreIds({ key: "size", ascending: true });
    await testId("message-list").click();
    await browser.keys("Home");
    await browser.keys("ArrowDown");
    const active = await browser.execute(() => document.querySelector('[data-testid="message-list"]')?.getAttribute("aria-activedescendant") ?? "");
    assert.ok(active.endsWith(`-${String(ids[1])}`), `cursor on ${active}, expected ${String(ids[1])}`);
    await browser.keys("Escape");
    // Back to newest first: the inbox's own list again, nothing stored for the view.
    await openViewMenu();
    await testId("sort-key-date").click();
    await openViewMenu();
    await testId("sort-descending").click();
    await browser.waitUntil(async () => (await browser.execute(() => document.querySelectorAll('[data-testid="list-section"]').length)) > 0, {
      timeout: 20000,
      timeoutMsg: "the date sections did not come back",
    });
    assert.equal((await coreCall("get_list_orders")).find((o) => o.view === viewKey()), undefined);
  });

  it("2: draws a table whose headers sort, whose columns resize and can be chosen", async () => {
    await openViewMenu();
    await testId("presentation-table").click();
    await testId("table-header").waitForExist({ timeout: 10000 });
    const headers = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid="table-header"] [role="columnheader"]')].map((h) => ({ name: h.getAttribute("aria-label"), sort: h.getAttribute("aria-sort") })),
    );
    report.steps.headers = headers;
    assert.deepEqual(
      headers.map((h) => h.name),
      ["Flagged", "Attachment", "Sender", "Subject", "Date", "Size"],
    );
    assert.equal(headers.find((h) => h.name === "Date")?.sort, "descending");
    assert.equal(await testId("message-list").getAttribute("role"), "listbox");
    report.screenshots.push(await screenshot("list-views-table-light"));

    // A click on "Subject" sorts A to Z, a second one Z to A.
    let ids = await coreIds({ key: "subject", ascending: true });
    let timed = await timedClick('[data-testid="sort-by-subject"]', ids[0]);
    report.timings["table header subject asc"] = timed.ms;
    await waitRows(ids, "table: subject A to Z");
    assert.equal(await $('[data-testid="column-subject"]').getAttribute("aria-sort"), "ascending");
    assert.equal(await $('[data-testid="column-date"]').getAttribute("aria-sort"), null);
    ids = await coreIds({ key: "subject", ascending: false });
    timed = await timedClick('[data-testid="sort-by-subject"]', ids[0]);
    report.timings["table header subject desc"] = timed.ms;
    await waitRows(ids, "table: subject Z to A");
    assert.equal(await $('[data-testid="column-subject"]').getAttribute("aria-sort"), "descending");

    // Header and rows line up, column by column.
    let lined = await alignment();
    for (const id of ["correspondent", "subject", "date", "size"]) {
      assert.ok(lined.header[id] && lined.cells[id], `no ${id} cell: ${JSON.stringify(lined)}`);
      assert.ok(Math.abs(lined.header[id].left - lined.cells[id].left) <= 1, `${id} is not aligned: ${JSON.stringify(lined)}`);
    }

    // A real drag of the sender column's handle: 80 px wider, rows follow, stored.
    const handle = await browser.execute(() => {
      const r = document.querySelector('[data-testid="column-resize-correspondent"]')?.getBoundingClientRect();
      return r ? { x: r.left + r.width / 2, y: r.top + r.height / 2 } : null;
    });
    assert.ok(handle, "no handle on the sender column");
    const before = Number(await testId("column-resize-correspondent").getAttribute("aria-valuenow"));
    await drag(handle, { x: handle.x + 80, y: handle.y });
    const after = Number(await testId("column-resize-correspondent").getAttribute("aria-valuenow"));
    report.steps.drag = { before, after };
    assert.ok(Math.abs(after - before - 80) <= 2, `dragged from ${String(before)} to ${String(after)}`);
    lined = await alignment();
    assert.ok(Math.abs(lined.header.subject.left - lined.cells.subject.left) <= 1, `subject not aligned after the drag: ${JSON.stringify(lined)}`);
    // The keyboard: → adds 16 px.
    await browser.execute(() => {
      document.querySelector('[data-testid="column-resize-correspondent"]')?.focus();
    });
    await browser.keys("ArrowRight");
    const keyed = Number(await testId("column-resize-correspondent").getAttribute("aria-valuenow"));
    assert.equal(keyed, after + 16);
    await browser.waitUntil(async () => (await coreCall("get_list_table")).columns.find((c) => c.id === "correspondent")?.width === keyed, {
      timeout: 5000,
      timeoutMsg: "the width was not stored",
    });

    // The view menu hides the size column.
    await openViewMenu();
    await testId("column-toggle-size").click();
    await browser.waitUntil(async () => !(await $('[data-testid="column-size"]').isExisting()), { timeout: 5000, timeoutMsg: "the size column stayed" });
    await closeMenus().catch(() => undefined);
    assert.equal((await coreCall("get_list_table")).columns.find((c) => c.id === "size")?.visible, false);
    await openViewMenu();
    await testId("column-toggle-size").click();
    await closeMenus().catch(() => undefined);
    await $('[data-testid="column-size"]').waitForExist({ timeout: 5000 });

    // Keyboard and screen reader: the list is still a listbox, ↓ walks its options.
    await testId("message-list").click();
    await browser.keys("Home");
    await browser.keys("ArrowDown");
    const active = await browser.execute(() => document.querySelector('[data-testid="message-list"]')?.getAttribute("aria-activedescendant") ?? "");
    assert.ok(active.endsWith(`-${String(ids[1])}`), `cursor on ${active}, expected ${String(ids[1])}`);
    await browser.keys("Escape");

    // With conversations on, the table's list is a tree of treeitems.
    await setConversations(true);
    await browser.waitUntil(async () => (await testId("message-list").getAttribute("role")) === "tree", { timeout: 15000, timeoutMsg: "no tree" });
    const tree = await browser.execute(() => {
      const items = [...document.querySelectorAll('[data-testid="message-list"] [role="treeitem"]')];
      return { items: items.length, levels: [...new Set(items.map((i) => i.getAttribute("aria-level")))], tableRows: document.querySelectorAll('[data-testid="message-list"] [data-testid="table-row"]').length };
    });
    report.steps.tree = tree;
    assert.ok(tree.items > 0 && tree.tableRows > 0, JSON.stringify(tree));
    assert.deepEqual(tree.levels, ["1"]);
    report.steps.axeLight = await seriousAxe();
    assert.deepEqual(report.steps.axeLight, [], `axe light: ${JSON.stringify(report.steps.axeLight)}`);
    await setTheme("dark");
    report.screenshots.push(await screenshot("list-views-table-dark"));
    report.steps.axeDark = await seriousAxe();
    assert.deepEqual(report.steps.axeDark, [], `axe dark: ${JSON.stringify(report.steps.axeDark)}`);
    await setTheme("light");
    await setConversations(false);
    await browser.waitUntil(async () => (await testId("message-list").getAttribute("role")) === "listbox", { timeout: 15000, timeoutMsg: "no listbox" });
  });

  it("3: the quick filter combines switches, a tag and text, counts like the core and resets", async () => {
    // Newest first again, through the menu (the window keeps the orders it has read).
    await openViewMenu();
    await testId("sort-key-date").click();
    await browser.waitUntil(async () => (await coreCall("get_list_orders")).find((o) => o.view === viewKey()) === undefined, {
      timeout: 5000,
      timeoutMsg: "the order was not reset",
    });
    // Three messages get the tag "Projekt" for this spec (writes on for alice meanwhile).
    await coreCall("set_writes_enabled", { accountId: account.id, enabled: true });
    tagged = (await coreIds({ key: "date", ascending: false }, NO_FILTER, 3)).slice(0, 3);
    await coreCall("apply_action", {
      action: { type: "addKeyword", keyword: "Projekt" },
      messages: tagged.map((id) => ({ mailboxId: inbox.id, messageId: id })),
      requestId: requestId(),
    });
    const count = (filter) => coreCall("count_list_view", { request: { scope: scope(), order: { key: "date", ascending: false }, filter } });
    const shownCount = async () => (await testId("quick-filter-count").isExisting()) ? await testId("quick-filter-count").getText() : null;
    const check = async (name, filter) => {
      const ids = await coreIds({ key: "date", ascending: false }, filter);
      const expected = await count(filter);
      if (ids.length > 0) await waitRows(ids, name);
      await browser.waitUntil(async () => (await shownCount())?.startsWith(expected.toLocaleString("en")), {
        timeout: 10000,
        timeoutMsg: `${name}: the bar does not say ${String(expected)} (${String(await shownCount())})`,
      });
      report.steps[`filter ${name}`] = { count: expected, first: ids.slice(0, 3) };
      return { ids, expected };
    };

    // Unread: stage 16's own list; the count from the core.
    let firstIds = await coreIds({ key: "date", ascending: false }, { ...NO_FILTER, unread: true });
    let timed = await timedClick('[data-testid="quick-filter-unread"]', firstIds[0]);
    report.timings["filter unread"] = timed.ms;
    await check("unread", { ...NO_FILTER, unread: true });
    // Unread and flagged.
    const flagged = { ...NO_FILTER, unread: true, flagged: true };
    const flaggedIds = await coreIds({ key: "date", ascending: false }, flagged);
    if (flaggedIds.length > 0) {
      timed = await timedClick('[data-testid="quick-filter-flagged"]', flaggedIds[0]);
      report.timings["filter unread + flagged"] = timed.ms;
    } else {
      await testId("quick-filter-flagged").click();
    }
    await check("unread + flagged", flagged);
    await testId("quick-filter-unread").click();
    await testId("quick-filter-flagged").click();
    // With attachment.
    firstIds = await coreIds({ key: "date", ascending: false }, { ...NO_FILTER, attachment: true });
    timed = await timedClick('[data-testid="quick-filter-attachment"]', firstIds[0]);
    report.timings["filter attachment"] = timed.ms;
    await check("attachment", { ...NO_FILTER, attachment: true });
    await testId("quick-filter-attachment").click();
    // A tag from the bar's menu.
    await browser.waitUntil(
      async () => (await count({ ...NO_FILTER, tag: "Projekt" })) === 3 && (await coreCall("get_outbox_counts", { accountId: account.id })).pending === 0,
      { timeout: 30000, timeoutMsg: "the tag did not arrive" },
    );
    await testId("quick-filter-tag").click();
    // The server may give the keyword back in lower case (Stalwart does): any spelling.
    const tagItem = async () =>
      browser.execute(() => [...document.querySelectorAll('[data-testid^="quick-filter-tag-"]')].find((el) => el.textContent.trim().toLowerCase() === "projekt")?.dataset.testid ?? null);
    await browser.waitUntil(async () => (await tagItem()) !== null, { timeout: 10000, timeoutMsg: "the tag is not in the menu" });
    report.steps.tagItem = await tagItem();
    await testId(await tagItem()).click();
    const tagResult = await check("tag", { ...NO_FILTER, tag: "Projekt" });
    assert.equal(tagResult.expected, 3);
    assert.deepEqual([...tagResult.ids].sort(), [...tagged].sort());
    await testId("quick-filter-tag").click();
    await testId("quick-filter-tag-none").click();
    // The tag goes again, on the server too, before writes are off again.
    await coreCall("apply_action", {
      action: { type: "removeKeyword", keyword: "Projekt" },
      messages: tagged.map((id) => ({ mailboxId: inbox.id, messageId: id })),
      requestId: requestId(),
    });
    await browser.waitUntil(
      async () => (await count({ ...NO_FILTER, tag: "Projekt" })) === 0 && (await coreCall("get_outbox_counts", { accountId: account.id })).pending === 0,
      { timeout: 30000, timeoutMsg: "the tag did not go" },
    );
    const outbox = await coreCall("get_outbox_counts", { accountId: account.id });
    assert.equal(outbox.failed + outbox.conflict, 0, `outbox: ${JSON.stringify(outbox)}`);
    tagged = [];
    await coreCall("set_writes_enabled", { accountId: account.id, enabled: false });
    // Text: search language, the last word as a prefix; combined with Unread.
    await testId("quick-filter-text").click();
    const started = Date.now();
    await browser.keys([..."invoic"]);
    const textResult = await check("text invoic", { ...NO_FILTER, text: "invoic" });
    report.timings["filter text (incl. 150 ms pause)"] = Date.now() - started;
    assert.ok(textResult.expected > 0, "no invoice in the seed");
    await testId("quick-filter-unread").click();
    await check("text + unread", { ...NO_FILTER, text: "invoic", unread: true });
    await testId("quick-filter-flagged").click();
    report.screenshots.push(await screenshot("list-views-quick-filter"));
    await testId("quick-filter-flagged").click();
    // Sorted and filtered at once.
    await openViewMenu();
    await testId("sort-key-sender").click();
    await waitRows(await coreIds({ key: "sender", ascending: true }, { ...NO_FILTER, text: "invoic", unread: true }), "filtered by sender");
    // Escape in the field clears the text, Reset the rest.
    await testId("quick-filter-text").click();
    await browser.keys("Escape");
    assert.equal(await testId("quick-filter-text").getValue(), "");
    await testId("quick-filter-reset").click();
    await testId("quick-filter-count").waitForExist({ reverse: true, timeout: 5000 });
    await waitRows(await coreIds({ key: "sender", ascending: true }), "reset, still by sender");
    await openViewMenu();
    await testId("sort-key-date").click();

    // Keyboard: Cmd/Ctrl+Shift+K puts the focus into the field; the switches are one toolbar.
    await testId("message-list").click();
    await browser.keys([MOD, "Shift", "k"]);
    const focused = await browser.execute(() => document.activeElement?.getAttribute("data-testid"));
    assert.equal(focused, "quick-filter-text");
    const stops = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid="quick-filter"] [role="toolbar"] button')].map((b) => b.tabIndex),
    );
    assert.equal(stops.filter((s) => s === 0).length, 1, `toolbar tab stops: ${JSON.stringify(stops)}`);
    report.steps.axeFilter = await seriousAxe();
    assert.deepEqual(report.steps.axeFilter, []);
  });
});
