// Stage 22, items 3 and 8 (decisions 0072, 0073) in the real app against the local test server, in
// German, light and dark:
// 1. alice's inbox: no avatar, no picture, no initials; every row starts with the narrow mark
//    column (unread dot, the box on hover) after the row's lead, on the line of the title and the
//    section labels; the sender 8 px after it; no account dot in a single account's list.
// 2. "Alle Posteingänge" with alice and bob: each row has its account's label (a dot in its colour
//    and its name, named by its tooltip) after the subject, away from the unread dot; it stays
//    while the quick actions show (real mouse events).
// 3. The narrow rail: the accounts are dots in their colour, the name as tooltip and accessible
//    name, no letters.
// 4. Contacts: rows and the contact's page without a picture.
// 5. The settings search: Cmd+F in the settings puts the focus into the field; "Signatur" lists
//    the hits grouped by section; the arrow keys move; Enter opens "Schreiben", scrolls to the row
//    and marks it; Escape clears; "Ton" finds the sound; no CSP violation.
//
// Sets up alice and bob itself and removes them at the end; appends `S22 <run>` messages to both
// INBOXes. Screenshots go to tests/e2e/screenshots/s22-*.png, the facts to s22-ui-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { $, $$, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openSettings, openView, screenshot, SCREENSHOT_DIR, setSidebar, testId } from "./support/app.mjs";
import { ALICE, SERVER, appendMessages, appendToBob } from "./support/testmail.mjs";

const BOB = { email: "bob@vicemail.test", password: "bob-test-pw" };
const RUN = Date.now().toString(36);
const MARK = `S22 ${RUN}`;
const MOD = process.platform === "darwin" ? "Meta" : "Control";
const report = { screenshots: [], steps: {} };
let alice = null;
let bob = null;

function eml(subject, from, to, index) {
  return [
    `Date: ${new Date(Date.now() - index * 60000).toUTCString().replace("GMT", "+0000")}`,
    `From: ${from}`,
    `To: ${to}`,
    `Subject: ${subject}`,
    `Message-ID: <s22-${RUN}-${String(index)}@e2e.vicemail.test>`,
    "MIME-Version: 1.0",
    "Content-Type: text/plain; charset=utf-8",
    "",
    `Hallo, das ist ${subject}.`,
    "",
  ].join("\r\n");
}

function writeMessages(entries, offset = 0) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "s22-ui-"));
  return entries.map(([subject, from, to], index) => {
    const file = path.join(dir, `m${String(index)}.eml`);
    fs.writeFileSync(file, eml(subject, from, to, offset + index + 1));
    return file;
  });
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

async function setTheme(theme) {
  await coreCall("set_preferences", { preferences: { theme, language: "de" } });
  await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme, {
    timeout: 10000,
    timeoutMsg: `no ${theme} theme`,
  });
}

/** Light and dark screenshots of what is on screen now. */
async function both(name) {
  for (const theme of ["light", "dark"]) {
    await setTheme(theme);
    await browser.pause(150);
    report.screenshots.push(await screenshot(`s22-${name}-${theme}`));
  }
  await setTheme("light");
}

/** The rows of the list with their parts (boxes in CSS pixels). */
const rows = () =>
  browser.execute(() => {
    const box = (el) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height };
    };
    const list = document.querySelector('[data-testid="message-list"]');
    const title = document.querySelector("[data-testid=\"app-toolbar\"] h1");
    const titleText = title ? (() => {
      const range = document.createRange();
      range.selectNodeContents(title);
      return range.getBoundingClientRect().left;
    })() : null;
    return {
      titleLeft: titleText,
      avatars: list ? list.querySelectorAll('[data-testid="avatar"], img').length : -1,
      rows: [...(list?.querySelectorAll('[data-testid="message-row"]') ?? [])].slice(0, 12).map((row) => {
        const label = row.querySelector('[data-testid="row-account"]');
        // Stage 23 (decision 0084): the account's stripe at the row's right edge.
        const dot = label;
        const actions = row.querySelector('[data-testid="row-actions"]');
        return {
          name: row.querySelector('[data-testid="row-name"]')?.textContent ?? "",
          mark: box(row.querySelector('[data-testid="row-mark"]')),
          sender: box(row.querySelector('[data-testid="row-sender"]')),
          unreadDot: box(row.querySelector('[data-testid="row-unread-dot"]')),
          subject: box(row.querySelector('[data-testid="row-subject"]')),
          accountDot: box(dot),
          accountTitle: label?.getAttribute("title") ?? null,
          accountColour: dot ? getComputedStyle(dot).backgroundColor : null,
          actions: actions && getComputedStyle(actions).display !== "none" ? box(actions) : null,
        };
      }),
    };
  });

describe("stage 22: no pictures, the mark column, account dots and the settings search", () => {
  before(async () => {
    await attach();
    await coreCall("set_preferences", { preferences: { theme: "light", language: "de" } });
    alice = await addAccount(ALICE, "Alice Archer");
    bob = await addAccount(BOB, "Bob Baker");
    await browser.waitUntil(
      async () => {
        const accounts = await coreCall("list_accounts");
        return accounts.length === 2 && accounts.every((a) => a.status.state === "idle" && a.status.lastSyncAt !== null);
      },
      { timeout: 300000, interval: 500, timeoutMsg: "the first syncs did not finish" },
    );
    if (await testId("update-consent-no").isExisting()) await testId("update-consent-no").click();
    await coreCall("set_reading_preferences", { preferences: { markReadOnOpen: "never" } });
    await appendMessages(
      "INBOX",
      writeMessages([
        [`${MARK} Projektplan`, "Grace Hopper <grace@example.org>", "Alice Archer <alice@vicemail.test>"],
        [`${MARK} Mittagessen`, "Ada Lovelace <ada@example.org>", "Alice Archer <alice@vicemail.test>"],
        [`${MARK} Rechnung November`, "Edsger Dijkstra <edsger@example.org>", "Alice Archer <alice@vicemail.test>"],
      ]),
    );
    // Bob's message is the newest (dated now): in a full run, earlier specs leave mail of the last
    // minutes in alice's inbox, and the unified inbox step looks at its first twelve rows.
    await appendToBob(writeMessages([[`${MARK} Vereinstreffen`, "Grace Hopper <grace@example.org>", "Bob Baker <bob@vicemail.test>"]], -1));
    await coreCall("retry_account", { accountId: alice.id });
    await coreCall("retry_account", { accountId: bob.id });
    await setSidebar(true);
  });

  after(async () => {
    await browser.keys(["Escape"]).catch(() => undefined);
    await coreCall("set_preferences", { preferences: { theme: "light", language: "en" } }).catch(() => undefined);
    await setSidebar(false).catch(() => undefined);
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "s22-ui-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("shows alice's inbox without avatars: the mark column first, on the title's line", async () => {
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
    await browser.waitUntil(async () => (await rows()).rows.some((r) => r.name.includes(`${MARK} Projektplan`)), {
      timeout: 60000,
      timeoutMsg: "the appended messages did not arrive",
    });
    const facts = await rows();
    const scale = await browser.execute(() => Number.parseFloat(getComputedStyle(document.documentElement).fontSize) / 16);
    report.steps.inbox = { scale, titleLeft: facts.titleLeft, rows: facts.rows.slice(0, 4) };
    assert.equal(facts.avatars, 0, "pictures or avatars in the list");
    for (const row of facts.rows) {
      assert.ok(row.mark, `no mark column in "${row.name}"`);
      assert.ok(Math.abs(row.mark.width - 16 * scale) < 1, `the mark column of "${row.name}" is ${String(row.mark.width)} px`);
      // Stage 23 (decision 0084): 4 px in narrow (several-line) rows, 8 px in wide ones.
      const after = row.sender.left - row.mark.right;
      assert.ok([4, 8].some((gap) => Math.abs(after - gap * scale) < 1.5), `the sender of "${row.name}" is ${String(after)} px after the mark`);
      assert.equal(row.accountDot, null, `an account dot in a single account's list ("${row.name}")`);
      if (row.unreadDot) assert.ok(row.unreadDot.left >= row.mark.left && row.unreadDot.right <= row.mark.right, "the unread dot is not in the mark column");
    }
    // Stage 23 (decision 0084): the title stands in the toolbar, 16 px into the list's segment, the
    // mark column 8 px into the row (the draft's dot 16 px from the list's edge): no common line.
    await both("inbox");
  });

  it("shows each row's account with its colour's dot in Alle Posteingänge, also on hover", async () => {
    const unified = await browser.$('[data-testid="unified-tree"] [role="treeitem"]');
    await unified.waitForExist({ timeout: 20000 });
    await unified.click();
    await browser.waitUntil(async () => (await unified.getAttribute("aria-current")) === "page", { timeoutMsg: "the unified inbox did not open" });
    await browser.waitUntil(async () => (await rows()).rows.some((r) => r.name.includes(`${MARK} Vereinstreffen`)), {
      timeout: 60000,
      timeoutMsg: "bob's message is not in the unified inbox",
    });
    const facts = await rows();
    report.steps.unified = facts.rows.slice(0, 6);
    assert.equal(facts.avatars, 0, "pictures or avatars in the list");
    const colours = new Map();
    for (const row of facts.rows) {
      assert.ok(row.accountDot, `no account dot in "${row.name}"`);
      assert.ok(["Alice Archer", "Bob Baker"].includes(row.accountTitle ?? ""), `account dot named ${String(row.accountTitle)}`);
      assert.ok(row.name.includes(`Konto: ${String(row.accountTitle)}`), `the row's name lacks the account: ${row.name}`);
      assert.ok(row.accountDot.left >= row.sender.right, `the account dot of "${row.name}" is not after the sender`);
      if (row.unreadDot) assert.ok(row.accountDot.left - row.unreadDot.right > 100, `the account dot of "${row.name}" is next to the unread dot`);
      colours.set(row.accountTitle, row.accountColour);
    }
    assert.equal(colours.size, 2, "both accounts in view");
    assert.notEqual(colours.get("Alice Archer"), colours.get("Bob Baker"), "both accounts in one colour");
    await both("unified");
    // Hovered (a real mouse move): the quick actions show, the account's dot stays before them.
    const first = facts.rows.find((r) => r.name.includes(`${MARK} Vereinstreffen`)) ?? facts.rows[0];
    const viewportHeight = await browser.execute(() => innerHeight);
    const y = (first.mark.top + first.mark.bottom) / 2;
    await coreCall("e2e_native_mouse_event", { kind: "move", x: first.sender.right + 40, y, viewportHeight });
    await browser.waitUntil(async () => (await rows()).rows.some((r) => r.actions !== null), { timeout: 5000, timeoutMsg: "no quick actions on hover" });
    const hovered = (await rows()).rows.find((r) => r.actions !== null);
    report.steps.unifiedHover = hovered;
    assert.ok(hovered.accountDot && hovered.accountDot.width > 0, "the account's dot went with the hover");
    // Stage 23 (decision 0084): the stripe at the row's edge, clear of the quick actions.
    assert.ok(hovered.accountDot.left >= hovered.actions.right, "the account's stripe is under the quick actions");
    report.screenshots.push(await screenshot("s22-unified-hover-light"));
    await coreCall("e2e_native_mouse_event", { kind: "move", x: 2, y: 2, viewportHeight });
  });

  it("shows the accounts in the narrow rail as dots in their colour, named by tooltip and name", async () => {
    await setSidebar(false);
    const accounts = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid^="account-switch-"]')].map((entry) => ({
        label: entry.getAttribute("aria-label"),
        text: entry.textContent,
        dot: Boolean(entry.querySelector('[data-testid="account-dot"]')),
        dotSize: entry.querySelector('[data-testid="account-dot"]')?.getBoundingClientRect().width ?? 0,
        avatar: Boolean(entry.querySelector('[data-testid="avatar"], img')),
      })),
    );
    report.steps.rail = accounts;
    assert.equal(accounts.length, 2);
    for (const account of accounts) {
      assert.ok(account.dot && account.dotSize > 0, `no dot for ${String(account.label)}`);
      assert.equal(account.avatar, false, `initials or a picture for ${String(account.label)}`);
      assert.equal(account.text, "", `letters on ${String(account.label)}`);
      assert.match(account.label ?? "", /Alice Archer|Bob Baker/);
    }
    // The tooltip names the account.
    const entry = await $('[data-testid^="account-switch-"]');
    await entry.moveTo();
    const tooltip = await $('[role="tooltip"]');
    await tooltip.waitForExist({ timeout: 5000 });
    report.steps.railTooltip = await tooltip.getText();
    assert.match(report.steps.railTooltip, /Alice Archer|Bob Baker/);
    await both("rail");
    await browser.execute(() => {
      if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    });
    await setSidebar(true);
  });

  it("lists contacts and shows a contact without a picture", async () => {
    await openView("contacts");
    await testId("contacts-view").waitForExist({ timeout: 10000 });
    await browser.waitUntil(async () => (await $$('[data-testid="contact-row"]')).length > 0, { timeout: 30000, timeoutMsg: "no contacts" });
    const facts = await browser.execute(() => {
      const view = document.querySelector('[data-testid="contacts-view"]');
      return {
        pictures: view.querySelectorAll('img, [data-testid="avatar"], [data-testid="contact-picture-area"]').length,
        rows: view.querySelectorAll('[data-testid="contact-row"]').length,
        detail: view.querySelector('[data-testid="contact-detail-name"]')?.textContent ?? null,
      };
    });
    report.steps.contacts = facts;
    assert.equal(facts.pictures, 0, "a picture in the contacts view");
    assert.ok(facts.detail, "no contact open");
    await both("contacts");
  });

  it("finds settings: Cmd+F, hits grouped by section, Enter jumps and marks the row, Escape clears", async () => {
    await openSettings("general");
    await browser.keys([MOD, "f"]);
    await browser.keys([MOD]);
    const field = await $('[role="combobox"][aria-label="Einstellungen durchsuchen"]');
    await browser.waitUntil(async () => (await browser.execute(() => document.activeElement?.getAttribute("aria-label"))) === "Einstellungen durchsuchen", {
      timeout: 5000,
      timeoutMsg: "Cmd+F did not focus the settings search",
    });
    await field.setValue("Signatur");
    const listed = await browser.execute(() => {
      const list = document.querySelector('[data-testid="settings-search-results"]');
      return {
        shown: Boolean(list && !list.hidden && list.getBoundingClientRect().height > 0),
        groups: [...(list?.querySelectorAll('[role="group"]') ?? [])].map((group) => document.getElementById(group.getAttribute("aria-labelledby") ?? "")?.textContent),
        hits: [...(list?.querySelectorAll('[role="option"]') ?? [])].map((option) => option.textContent),
        expanded: document.activeElement?.getAttribute("aria-expanded"),
        active: document.activeElement?.getAttribute("aria-activedescendant"),
        status: document.querySelector('[data-testid="settings-search-status"]')?.textContent,
      };
    });
    report.steps.searchSignatur = listed;
    assert.ok(listed.shown, "no hits shown");
    assert.deepEqual(listed.groups, ["Schreiben"]);
    assert.ok(listed.hits.some((hit) => hit.startsWith("Signaturen")), JSON.stringify(listed.hits));
    assert.equal(listed.expanded, "true");
    assert.ok(listed.active, "no active hit");
    await both("settings-search");
    // The list survives the theme switch; the field still has the focus for the keys.
    await field.click();
    await browser.keys(["ArrowDown", "ArrowUp", "Enter"]);
    await browser.waitUntil(async () => (await testId("settings-tab-compose").getAttribute("data-state")) === "active", {
      timeout: 5000,
      timeoutMsg: "Enter did not open Schreiben",
    });
    const jumped = await browser.execute(() => {
      const row = document.querySelector('[data-settings-panel="compose"] [data-setting-found]');
      const r = row?.getBoundingClientRect();
      return {
        title: row?.getAttribute("data-setting-title") ?? null,
        visible: Boolean(r && r.top >= 0 && r.bottom <= innerHeight),
        background: row ? getComputedStyle(row).backgroundColor : null,
        focusInside: Boolean(row && (row === document.activeElement || row.contains(document.activeElement))),
      };
    });
    report.steps.jump = jumped;
    assert.equal(jumped.title, "Signaturen");
    assert.ok(jumped.visible, "the row is not in view");
    assert.ok(jumped.focusInside, "the focus is not at the row");
    report.screenshots.push(await screenshot("s22-settings-jump-light"));
    await browser.waitUntil(
      () => browser.execute(() => document.querySelector('[data-settings-panel="compose"] [data-setting-found]') === null),
      { timeout: 5000, timeoutMsg: "the mark did not go" },
    );
    // "Ton" finds the sound of notifications; Escape clears the field.
    await field.click();
    await field.setValue("Ton");
    const ton = await browser.execute(() => [...document.querySelectorAll('[data-testid="settings-search-results"] [role="option"]')].map((option) => option.textContent));
    report.steps.searchTon = ton;
    assert.ok(ton.some((hit) => hit.startsWith("Ton abspielen")), JSON.stringify(ton));
    await browser.keys(["Escape"]);
    assert.equal(await field.getValue(), "");
    assert.deepEqual(await cspViolations(), [], "CSP violations");
  });
});
