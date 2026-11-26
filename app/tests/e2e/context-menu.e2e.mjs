// Stage 21 (items 4a, 5 and 6; decisions 0064 and 0065) in the real app against the local test
// server, with real mouse events (`e2e_native_mouse_event`: NSEvents handed to the WKWebView, a
// secondary click for `contextmenu`):
// 1. A right click on a row of alice's inbox opens the message menu at the pointer (never the web
//    view's own menu); the row, outside the selection, gets a ring and stays unselected. "Move to"
//    opens by keyboard on its first folder; Escape closes the submenu, then the menu, and the list
//    has the focus again.
// 2. "Delete" from the menu moves the message to the trash on the server (alice may write).
// 3. In "All inboxes" (alice and bob, bob read-only) hovering a row shows its quick actions, with
//    its own account: alice's enabled, bob's disabled with the reason; bob's menu shows Delete
//    disabled with the reason (dark theme).
// 4. Stage 22 (decision 0072): the rows have no avatar; the narrow mark column starts at the row's
//    lead with the unread dot, the sender 8 px after it, the selection box only on hover, in the
//    dot's place; in "All inboxes" the account's label (dot and name) stays on hover, before the
//    quick actions.
// 5. No CSP violation.
//
// Sets up its own accounts (alice writable, bob read-only) and removes them at the end; appends
// `S21 menu <run>` messages to alice's and bob's INBOX. Screenshots go to
// tests/e2e/screenshots/s21-*.png, the facts to s21-context-menu-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, screenshot, SCREENSHOT_DIR, setSidebar, testId } from "./support/app.mjs";
import { ALICE, SERVER, appendMessages, appendToBob, folderCount, serverFolders } from "./support/testmail.mjs";

const BOB = { email: "bob@vicemail.test", password: "bob-test-pw" };
const RUN = Date.now().toString(36);
const MARK = `S21 menu ${RUN}`;
const report = { screenshots: [], steps: {}, mouse: [] };
let alice = null;
let bob = null;

function eml(subject, from, to, index) {
  return [
    `Date: ${new Date(Date.now() - index * 60000).toUTCString().replace("GMT", "+0000")}`,
    `From: ${from}`,
    `To: ${to}`,
    `Subject: ${subject}`,
    `Message-ID: <s21-${RUN}-${String(index)}@e2e.vicemail.test>`,
    "MIME-Version: 1.0",
    "Content-Type: text/plain; charset=utf-8",
    "",
    `Hello, this is ${subject}.`,
    "",
  ].join("\r\n");
}

function writeMessages(entries) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "s21-menu-"));
  return entries.map(([subject, from, to], index) => {
    const file = path.join(dir, `m${String(index)}.eml`);
    fs.writeFileSync(file, eml(subject, from, to, index + 1));
    return file;
  });
}

/** One real mouse event at a point of the viewport (CSS pixels), as columns.e2e.mjs sends them. */
async function mouse(kind, point) {
  const viewportHeight = await browser.execute(() => innerHeight);
  const result = await coreCall("e2e_native_mouse_event", { kind, x: point.x, y: point.y, viewportHeight });
  report.mouse = [...report.mouse.slice(-20), result];
  return result;
}

async function addAccount(who, displayName) {
  const added = await coreCall("add_account", {
    settings: {
      email: who.email,
      displayName,
      imap: { host: SERVER.host, port: SERVER.imapsPort, security: "tls", username: who.email },
      smtp: { host: SERVER.host, port: SERVER.smtpsPort, security: "tls", username: who.email },
    },
    password: who.password,
  });
  // Stage 22 (decision 0078): a new account shows its address until it is named in the lists.
  return coreCall("rename_account", { accountId: added.id, name: displayName });
}

/** The row (option or tree item) whose name has `subject`: its id, box and parts. */
const rowFacts = (subject) =>
  browser.execute((mark) => {
    const option = [...document.querySelectorAll('[data-testid="message-list"] [data-testid="message-row"]')].find((row) =>
      row.querySelector('[data-testid="row-name"]')?.textContent?.includes(mark),
    );
    if (!option) return null;
    const box = (el) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height };
    };
    const actions = option.querySelector('[data-testid="row-actions"]');
    const archive = option.querySelector('[data-testid="quick-archive"]');
    const check = option.querySelector('[data-testid="row-check"]');
    return {
      id: option.id,
      selected: option.getAttribute("aria-selected"),
      row: box(option),
      content: box(option.querySelector("[data-context-target], .rounded-row")),
      contentRing: option.querySelector("[data-context-target]") !== null,
      avatar: option.querySelector('[data-testid="avatar"]') !== null,
      mark: box(option.querySelector('[data-testid="row-mark"]')),
      dot: box(option.querySelector('[data-testid="row-unread-dot"]')),
      dotShown: option.querySelector('[data-testid="row-unread-dot"]') ? getComputedStyle(option.querySelector('[data-testid="row-unread-dot"]')).display !== "none" : null,
      checkShown: check ? getComputedStyle(check).display !== "none" : null,
      checkBox: box(check),
      senderBox: box(option.querySelector('[data-testid="row-sender"]')),
      // Stage 23 (decision 0084): the account's stripe at the row's right edge.
      accountDot: box(option.querySelector('[data-testid="row-account"]')),
      actionsBox: box(actions),
      sender: option.querySelector('[data-testid="row-sender"]') ? getComputedStyle(option.querySelector('[data-testid="row-sender"]')).fontWeight : null,
      hovered: option.matches(":hover"),
      actionsShown: actions ? getComputedStyle(actions).display !== "none" : false,
      targets: actions ? actions.querySelectorAll('[data-testid^="quick-"]').length : 0,
      archiveDisabled: archive?.getAttribute("aria-disabled") ?? null,
      archiveTitle: archive?.getAttribute("title") ?? null,
    };
  }, subject);

async function scrollToRow(subject) {
  await browser.waitUntil(async () => (await rowFacts(subject)) !== null, { timeout: 60000, timeoutMsg: `no row ${subject}` });
  const { id } = await rowFacts(subject);
  await browser.execute((rowId) => {
    document.getElementById(rowId)?.scrollIntoView({ block: "center" });
  }, id);
  await browser.pause(150);
  return rowFacts(subject);
}

/** The open message menu: its box, the focused item, and its submenus. */
const menuFacts = () =>
  browser.execute(() => {
    const menus = [...document.querySelectorAll('[data-testid="message-menu"] [role="menu"]')];
    const box = (el) => {
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height };
    };
    const active = document.activeElement;
    return {
      menus: menus.map((menu) => ({ label: menu.getAttribute("aria-label"), box: box(menu), depth: menu.getAttribute("data-menu-depth") })),
      focused: active?.getAttribute("data-menu-entry") ?? null,
      focusedIn: active?.closest('[role="menu"]')?.getAttribute("aria-label") ?? null,
      listFocused: active?.getAttribute("data-testid") === "message-list",
      window: { width: innerWidth, height: innerHeight },
    };
  });

/** Right-clicks the row of `subject` at 60 % of its width; waits for the menu. */
async function openMenu(subject) {
  const row = await scrollToRow(subject);
  const point = { x: row.row.left + row.row.width * 0.6, y: row.row.top + row.row.height / 2 };
  await mouse("move", point);
  await mouse("rightClick", point);
  await browser.waitUntil(async () => (await menuFacts()).menus.length > 0, { timeout: 10000, timeoutMsg: `no menu on ${subject}` });
  return { point, facts: await menuFacts() };
}

async function closeMenus() {
  for (let i = 0; i < 3 && (await menuFacts()).menus.length > 0; i += 1) {
    await browser.keys(["Escape"]);
    await browser.pause(100);
  }
}

describe("stage 21: the context menu of messages and the quick actions of mixed lists", () => {
  before(async () => {
    await attach();
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } });
    alice = await addAccount(ALICE, "Alice Archer");
    bob = await addAccount(BOB, "Bob Baker");
    await browser.waitUntil(
      async () => {
        const accounts = await coreCall("list_accounts");
        return accounts.length === 2 && accounts.every((a) => a.status.state === "idle" && a.status.lastSyncAt !== null);
      },
      { timeout: 300000, interval: 500, timeoutMsg: "the first syncs did not finish" },
    );
    // alice may write, bob stays read-only (new accounts only read).
    await coreCall("set_writes_enabled", { accountId: alice.id, enabled: true });
    await coreCall("set_reading_preferences", { preferences: { markReadOnOpen: "never" } });
    const no = await $("button=No, thanks");
    if (await no.isExisting()) await no.click();
    await appendMessages(
      "INBOX",
      writeMessages([
        [`${MARK} one`, "Grace Hopper <grace@example.org>", "Alice Archer <alice@vicemail.test>"],
        [`${MARK} two`, "Ada Lovelace <ada@example.org>", "Alice Archer <alice@vicemail.test>"],
        [`${MARK} three`, "Edsger Dijkstra <edsger@example.org>", "Alice Archer <alice@vicemail.test>"],
      ]),
    );
    await appendToBob(writeMessages([[`${MARK} bob`, "Grace Hopper <grace@example.org>", "Bob Baker <bob@vicemail.test>"]]));
    await coreCall("retry_account", { accountId: alice.id });
    await coreCall("retry_account", { accountId: bob.id });
    await setSidebar(true);
  });

  // A step that failed with its menu open leaves no menu to the next one.
  afterEach(async () => {
    await closeMenus().catch(() => undefined);
  });

  after(async () => {
    await closeMenus().catch(() => undefined);
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } }).catch(() => undefined);
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "s21-context-menu-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify({ ...report, mouse: report.mouse.slice(-5) }, null, 2)}\n`);
  });

  it("opens the menu at the pointer on a right click, a submenu by keyboard, and closes back to the list", async () => {
    // alice's inbox from her tree in the column.
    await browser.waitUntil(
      () =>
        browser.execute(() => {
          const tree = [...document.querySelectorAll('[role="tree"]')].find((t) => (t.getAttribute("aria-label") ?? "").includes("Alice"));
          const inbox = tree?.querySelector('[data-folder-role="inbox"]');
          inbox?.click();
          return inbox?.getAttribute("aria-current") === "page";
        }),
      { timeout: 20000, timeoutMsg: "alice's inbox did not open" },
    );
    const before = await scrollToRow(`${MARK} three`);
    assert.notEqual(before.selected, "true", "the row is selected already");
    const { point, facts } = await openMenu(`${MARK} three`);
    const root = facts.menus[0];
    report.steps.menu = { point, root, window: facts.window };
    // At the pointer: to its right and below, or flipped to the left or above near an edge.
    const inside = (low, high, size) => low >= 3.5 && high <= size - 3.5;
    assert.ok(Math.abs(root.box.left - point.x) <= 1 || Math.abs(root.box.right - point.x) <= 1, `menu x ${String(root.box.left)} vs pointer ${String(point.x)}`);
    assert.ok(
      Math.abs(root.box.top - point.y) <= 1 || Math.abs(root.box.bottom - point.y) <= 1 || inside(root.box.top, root.box.bottom, facts.window.height),
      `menu y ${String(root.box.top)}–${String(root.box.bottom)} vs pointer ${String(point.y)}`,
    );
    assert.ok(inside(root.box.left, root.box.right, facts.window.width) && inside(root.box.top, root.box.bottom, facts.window.height), "the menu leaves the window");
    assert.equal(root.label, "Actions for this message");
    assert.equal(facts.focused, "reply", "the first icon has the focus");
    // The row the menu acts on has the ring and stays unselected.
    const target = await rowFacts(`${MARK} three`);
    assert.ok(target.contentRing, "no ring on the targeted row");
    assert.notEqual(target.selected, "true", "the right click selected the row");
    report.screenshots.push(await screenshot("s21-menu-light"));
    // The screenshot takes the focus away (no focus ring in the picture): back on the first icon.
    await browser.execute(() => document.querySelector('[data-testid="message-menu"] [data-menu-entry="reply"]')?.focus());

    // Type-ahead to "Move to", → opens it on its first entry.
    await browser.keys(["m", "o"]);
    await browser.waitUntil(async () => (await menuFacts()).focused === "moveTo", { timeout: 5000, timeoutMsg: `type-ahead: ${JSON.stringify(await menuFacts())}` });
    await browser.keys(["ArrowRight"]);
    await browser.waitUntil(async () => (await menuFacts()).focusedIn === "Move to", { timeout: 5000, timeoutMsg: "the submenu did not take the focus" });
    let state = await menuFacts();
    assert.equal(state.menus.length, 2);
    const sub = state.menus[1];
    assert.ok(Math.abs(sub.box.left - root.box.right) <= 1 || Math.abs(sub.box.right - root.box.left) <= 1, `submenu beside the menu: ${JSON.stringify(state.menus)}`);
    assert.ok(sub.box.bottom <= state.window.height + 0.5, "the submenu leaves the window");
    report.steps.submenu = state;
    report.screenshots.push(await screenshot("s21-menu-submenu"));
    await browser.execute(() => document.querySelector('[data-testid="message-menu"] [role="menu"][data-menu-depth="1"] [data-menu-item]')?.focus());
    await browser.keys(["ArrowDown"]);
    state = await menuFacts();
    assert.equal(state.focusedIn, "Move to");
    // Escape: the submenu closes onto its item, then the menu onto the list.
    await browser.keys(["Escape"]);
    await browser.waitUntil(async () => (await menuFacts()).menus.length === 1, { timeout: 5000, timeoutMsg: "the submenu stayed" });
    assert.equal((await menuFacts()).focused, "moveTo");
    await browser.keys(["Escape"]);
    await browser.waitUntil(async () => (await menuFacts()).menus.length === 0, { timeout: 5000, timeoutMsg: "the menu stayed" });
    assert.ok((await menuFacts()).listFocused, "the list did not get the focus back");
    const after = await rowFacts(`${MARK} three`);
    assert.ok(!after.contentRing, "the ring stayed");
    assert.notEqual(after.selected, "true", "closing the menu selected the row");
  });

  it("deletes from the menu: the message lands in the trash on the server", async () => {
    const folders = await serverFolders();
    const trash = Object.keys(folders).find((name) => (folders[name].flags ?? []).some((flag) => /\\Trash/i.test(flag))) ?? "Trash";
    report.steps.trashFolder = trash;
    assert.equal((await folderCount("INBOX", `${MARK} two`)).count, 1);
    await openMenu(`${MARK} two`);
    await testId("menu-delete").click();
    await browser.waitUntil(async () => (await menuFacts()).menus.length === 0, { timeout: 5000, timeoutMsg: "the menu stayed after Delete" });
    await browser.waitUntil(async () => (await rowFacts(`${MARK} two`)) === null, { timeout: 20000, timeoutMsg: "the row stayed in the list" });
    await browser.waitUntil(async () => (await folderCount(trash, `${MARK} two`)).count === 1, {
      timeout: 60000,
      interval: 1000,
      timeoutMsg: `the message did not reach ${trash} on the server`,
    });
    assert.equal((await folderCount("INBOX", `${MARK} two`)).count, 0, "the message is still in the INBOX on the server");
    report.steps.deleted = { trash, inInbox: 0 };
  });

  it("starts the rows with the narrow mark column: the dot in it, the box in its place on hover", async () => {
    await mouse("move", { x: 2, y: 2 });
    const facts = await scrollToRow(`${MARK} one`);
    const scale = await browser.execute(() => Number.parseFloat(getComputedStyle(document.documentElement).fontSize) / 16);
    const lead = facts.mark.left - facts.content.left;
    const toSender = facts.senderBox.left - facts.mark.right;
    report.steps.rows = { lead, toSender, scale, facts };
    assert.equal(facts.avatar, false, "a row still has an avatar");
    assert.ok(lead >= 15 * scale && lead <= 17 * scale, `the mark column starts ${String(lead)} px into the row`);
    assert.ok(Math.abs(facts.mark.width - 16 * scale) < 1, `the mark column is ${String(facts.mark.width)} px wide`);
    assert.ok(Math.abs(toSender - 8 * scale) < 1, `the sender starts ${String(toSender)} px after the mark column`);
    assert.ok(facts.dot, "no unread dot on an unread row");
    assert.ok(facts.dot.left >= facts.mark.left && facts.dot.right <= facts.mark.right, `the dot is not in the mark column: ${JSON.stringify(facts)}`);
    assert.equal(facts.sender, "600", "unread rows are not bold");
    assert.equal(facts.checkShown, false, "the box shows at rest");
    report.screenshots.push(await screenshot("s22-rows"));
    // Hovered: the box takes the dot's place in the column; unread stays bold.
    await mouse("move", { x: facts.row.left + facts.row.width * 0.5, y: facts.row.top + facts.row.height / 2 });
    await browser.waitUntil(async () => (await rowFacts(`${MARK} one`)).checkShown === true, { timeout: 5000, timeoutMsg: "no selection box on hover" });
    const hovered = await rowFacts(`${MARK} one`);
    assert.equal(hovered.dotShown, false, "the dot shows under the box");
    assert.ok(hovered.checkBox.left >= hovered.mark.left - 0.5 && hovered.checkBox.right <= hovered.mark.right + 0.5, "the box is not in the mark column");
    report.screenshots.push(await screenshot("s22-rows-hover"));
    await mouse("move", { x: 2, y: 2 });
  });

  it("shows the quick actions in All inboxes with each row's account; bob's are disabled with the reason", async () => {
    const unified = await browser.$('[data-testid="unified-tree"] [role="treeitem"]');
    await unified.waitForExist({ timeout: 20000 });
    await unified.click();
    await browser.waitUntil(async () => (await unified.getAttribute("aria-current")) === "page", { timeoutMsg: "the unified inbox did not open" });
    const own = await scrollToRow(`${MARK} one`);
    await mouse("move", { x: own.row.left + own.row.width * 0.4, y: own.row.top + own.row.height / 2 });
    await browser.waitUntil(async () => (await rowFacts(`${MARK} one`)).actionsShown, { timeout: 5000, timeoutMsg: "no quick actions on alice's row" });
    const alicesRow = await rowFacts(`${MARK} one`);
    assert.ok(alicesRow.targets >= 4, `only ${String(alicesRow.targets)} quick actions`);
    // Stage 22: the account's label stays while the actions show, before them.
    assert.ok(alicesRow.accountDot && alicesRow.accountDot.width > 0, "no account dot on hover");
    assert.ok(alicesRow.accountDot.left >= alicesRow.actionsBox.right, "the account stripe is under the quick actions");
    assert.equal(alicesRow.archiveDisabled, null, "alice's archive is disabled");
    report.steps.unifiedAlice = alicesRow;
    report.screenshots.push(await screenshot("s21-unified-hover"));

    const bobs = await scrollToRow(`${MARK} bob`);
    await mouse("move", { x: bobs.row.left + bobs.row.width * 0.4, y: bobs.row.top + bobs.row.height / 2 });
    await browser.waitUntil(async () => (await rowFacts(`${MARK} bob`)).actionsShown, { timeout: 5000, timeoutMsg: "no quick actions on bob's row" });
    const bobsRow = await rowFacts(`${MARK} bob`);
    assert.equal(bobsRow.archiveDisabled, "true", "bob's archive is not disabled");
    assert.match(bobsRow.archiveTitle ?? "", /changes are turned off for this account/);
    report.steps.unifiedBob = bobsRow;
    report.screenshots.push(await screenshot("s21-unified-hover-read-only"));
    await mouse("move", { x: 2, y: 2 });
  });

  it("shows Delete disabled with the reason in a read-only account's menu (dark)", async () => {
    await coreCall("set_preferences", { preferences: { theme: "dark", language: "en" } });
    await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === "dark", { timeout: 10000, timeoutMsg: "no dark theme" });
    await openMenu(`${MARK} bob`);
    const facts = await browser.execute(() => {
      const item = document.querySelector('[data-testid="menu-delete"]');
      const reason = document.getElementById(item?.getAttribute("aria-describedby") ?? "");
      const allow = document.querySelector('[data-testid="menu-allowChanges"]');
      return {
        disabled: item?.getAttribute("aria-disabled") ?? null,
        reason: reason?.textContent ?? null,
        title: item?.getAttribute("title") ?? null,
        allow: allow?.textContent ?? null,
        pin: document.querySelector('[data-testid="menu-togglePin"], [data-testid="menu-pin"]')?.getAttribute("aria-disabled") ?? "enabled",
      };
    });
    report.steps.readOnlyMenu = facts;
    assert.equal(facts.disabled, "true");
    assert.match(facts.reason ?? "", /Read-only/);
    assert.match(facts.title ?? "", /changes are turned off/);
    assert.match(facts.allow ?? "", /Allow changes/);
    assert.equal(facts.pin, "enabled", "a local mark is disabled on a read-only account");
    report.screenshots.push(await screenshot("s21-menu-dark-read-only"));
    // Choosing it does nothing: the menu stays, nothing is sent.
    await testId("menu-delete").click();
    assert.equal((await menuFacts()).menus.length, 1, "the disabled item closed the menu");
    await closeMenus();
    // The same menu on alice's row in dark, for the record.
    await openMenu(`${MARK} three`);
    report.screenshots.push(await screenshot("s21-menu-dark"));
    await closeMenus();
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } });
    assert.deepEqual(await cspViolations(), [], "CSP violations");
  });
});
