// Stage 17 (decision 0049) in the real app: conversations unfold in the list like in Thunderbird,
// the reading pane shows exactly one message (docs/stages/17, block A).
//
// On the seed's 60-message thread "Planning the 2026 offsite" (Archive, INBOX and Sent) in the
// Archive folder, with the conversation view on (account-sync.e2e.mjs turned it off; this spec
// turns it off again at the end, the later specs count messages):
// 1. The list is a tree. The conversation row says "n replies" with a chevron; a click unfolds its
//    messages below it, one level deeper, on a dotted line, the user's own answers named "Sent",
//    others "Inbox"; one line each, in the child row height of the tokens. axe finds nothing
//    serious or critical in the unfolded tree, light and dark.
// 2. The conversation row opens one message on the right, a child row its own; never the stacked
//    conversation.
// 3. The keyboard alone: → unfolds and enters, ↓ walks the messages, ← goes up to the conversation
//    and folds it.
// 4. The list scrolls smoothly with the conversation unfolded (docs/SPEC.md: 60 frames per second,
//    no blank frame).
//
// Screenshots go to tests/e2e/screenshots/conversations-*.png, the facts to
// conversations-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openView, screenshot, SCREENSHOT_DIR, setConversations, testId } from "./support/app.mjs";
import { measureScroll } from "./support/scroll.mjs";
import { ALICE, SERVER } from "./support/testmail.mjs";

const require = createRequire(import.meta.url);
const AXE_SOURCE = fs.readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");

const report = { screenshots: [], steps: {} };
let account;
let conversationId = null;
/** The theme and language before this spec, put back at its end. */
let preferencesBefore = null;

/** Light or dark through the core, as a11y.e2e.mjs does: the list keeps its state meanwhile. */
async function setTheme(theme) {
  await coreCall("set_preferences", { preferences: { ...preferencesBefore, theme } });
  await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme, {
    timeoutMsg: `the ${theme} theme was not applied`,
  });
}

async function onlyAccount() {
  const accounts = await coreCall("list_accounts");
  assert.equal(accounts.length, 1, JSON.stringify(accounts.map((a) => a.email)));
  return accounts[0];
}

/** The tree item the list's cursor is on (`aria-activedescendant`). */
const cursor = () =>
  browser.execute(() => {
    const list = document.querySelector('[data-testid="message-list"]');
    const item = document.getElementById(list?.getAttribute("aria-activedescendant") ?? "");
    return item
      ? {
          id: item.id,
          level: item.getAttribute("aria-level"),
          posinset: item.getAttribute("aria-posinset"),
          setsize: item.getAttribute("aria-setsize"),
          expanded: item.getAttribute("aria-expanded"),
          selected: item.getAttribute("aria-selected"),
          name: item.querySelector('[data-testid="row-name"]')?.textContent ?? "",
        }
      : null;
  });

/** The conversation row's state and the child rows the list rendered. */
const tree = (id) =>
  browser.execute((rowId) => {
    const list = document.querySelector('[data-testid="message-list"]');
    const row = document.getElementById(rowId);
    const children = [...(list?.querySelectorAll('[data-testid="child-row"]') ?? [])];
    return {
      role: list?.getAttribute("role") ?? null,
      expanded: row?.getAttribute("aria-expanded") ?? null,
      level: row?.getAttribute("aria-level") ?? null,
      replies: row?.querySelector('[data-testid="row-replies"]')?.textContent ?? null,
      children: children.map((child) => ({
        level: child.getAttribute("aria-level"),
        posinset: Number(child.getAttribute("aria-posinset")),
        setsize: Number(child.getAttribute("aria-setsize")),
        folder: child.querySelector('[data-testid="child-folder"]')?.textContent ?? null,
        height: Math.round(child.getBoundingClientRect().height),
        name: child.querySelector('[data-testid="row-name"]')?.textContent ?? "",
      })),
      line: (() => {
        const line = list?.querySelector('[data-testid="child-row"] [data-testid="tree-line"]');
        if (!line) return null;
        const style = getComputedStyle(line);
        return { style: style.borderInlineStartStyle, width: style.borderInlineStartWidth };
      })(),
    };
  }, id);

async function runAxe() {
  const present = await browser.execute(() => typeof window.axe?.run === "function");
  if (!present) await browser.execute(`${AXE_SOURCE}\n;return typeof window.axe;`);
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

async function waitExpanded(value) {
  await browser.waitUntil(async () => (await tree(conversationId)).expanded === value, {
    timeout: 10000,
    timeoutMsg: `the conversation did not become aria-expanded=${value}`,
  });
}

async function waitChildren(present) {
  await browser.waitUntil(async () => (await tree(conversationId)).children.length > 0 === present, {
    timeout: 15000,
    timeoutMsg: present ? "the conversation's messages did not appear" : "the conversation's messages did not go",
  });
}

describe("stage 17: conversations unfold in the list, one message on the right", () => {
  before(async () => {
    await attach();
    preferencesBefore = await coreCall("get_preferences");
    await coreCall("add_account", {
      settings: {
        email: ALICE.email,
        displayName: "Alice Archer",
        imap: { host: SERVER.host, port: SERVER.imapsPort, security: "tls", username: ALICE.email },
        smtp: { host: SERVER.host, port: SERVER.smtpsPort, security: "tls", username: ALICE.email },
      },
      password: ALICE.password,
    });
    await browser.waitUntil(
      async () => {
        const a = await onlyAccount();
        return a.status.state === "idle" && a.status.lastSyncAt !== null;
      },
      { timeout: 240000, interval: 500, timeoutMsg: "the first sync did not finish" },
    );
    account = await onlyAccount();
    await setConversations(true);
    await openView("archive");
    await testId("message-list").waitForExist({ timeout: 20000 });
    await browser.waitUntil(async () => (await testId("message-list").getAttribute("role")) === "tree", {
      timeout: 20000,
      timeoutMsg: "the list of conversations is no tree",
    });
    await browser.waitUntil(
      async () => {
        conversationId = await browser.execute(() => {
          const rows = [...document.querySelectorAll('[data-testid="message-list"] [role="treeitem"][aria-level="1"]')];
          const offsite = rows.find((row) => row.querySelector('[data-testid="row-name"]')?.textContent.includes("offsite"));
          return (offsite ?? rows.find((row) => row.querySelector('[data-testid="row-replies"]')))?.id ?? null;
        });
        return conversationId !== null;
      },
      { timeout: 20000, timeoutMsg: "no conversation row with replies in Archive" },
    );
  });

  after(async () => {
    if (preferencesBefore) await coreCall("set_preferences", { preferences: preferencesBefore }).catch(() => undefined);
    await setConversations(false).catch(() => undefined);
    const left = await coreCall("list_accounts").catch(() => []);
    for (const a of left) await coreCall("remove_account", { accountId: a.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "conversations-report.json"), JSON.stringify(report, null, 2));
    console.log(`\nE2E REPORT ${JSON.stringify(report, null, 2)}\n`);
  });

  it("unfolds a conversation with the mouse: its messages below it, one level deeper, from every folder", async () => {
    const before = await tree(conversationId);
    report.steps.before = { role: before.role, expanded: before.expanded, level: before.level, replies: before.replies };
    assert.equal(before.role, "tree");
    assert.equal(before.expanded, "false");
    assert.equal(before.level, "1");
    assert.match(before.replies ?? "", /^\d+ repl(y|ies)$/);
    const replies = Number(/^\d+/.exec(before.replies ?? "")?.[0] ?? 0);

    await (await browser.$(`[id="${conversationId}"] [data-testid="row-replies"]`)).click();
    await waitExpanded("true");
    await waitChildren(true);
    const open = await tree(conversationId);
    report.steps.unfolded = { children: open.children.slice(0, 6), rendered: open.children.length, line: open.line };
    const first = open.children[0];
    assert.equal(first.level, "2");
    assert.equal(first.posinset, 1);
    assert.equal(first.setsize, Math.min(replies + 1, 1000), "every message of the conversation, the row's count");
    // Messages from other folders say which; this one lies in Archive, others in Sent and Inbox.
    const folders = new Set(open.children.map((child) => child.folder).filter(Boolean));
    report.steps.folders = [...folders];
    assert.ok(folders.has("Sent"), `no message from Sent among ${JSON.stringify([...folders])}`);
    // One line each, in the token's height, on a fine dotted line.
    assert.ok(open.children.every((child) => child.height === 36), JSON.stringify(open.children.map((c) => c.height)));
    assert.deepEqual(open.line, { style: "dotted", width: "1px" });
    report.screenshots.push(await screenshot("conversations-unfolded-light"));
    const lightFindings = await runAxe();
    assert.deepEqual(lightFindings, [], `axe, light: ${JSON.stringify(lightFindings)}`);
    await setTheme("dark");
    report.screenshots.push(await screenshot("conversations-unfolded-dark"));
    const darkFindings = await runAxe();
    assert.deepEqual(darkFindings, [], `axe, dark: ${JSON.stringify(darkFindings)}`);
    await setTheme("light");
  });

  it("shows one message on the right: the conversation row one, a message row its own", async () => {
    await (await browser.$(`[id="${conversationId}"]`)).click();
    await testId("thread-pane").waitForExist({ timeout: 10000 });
    await testId("message-card").waitForExist({ timeout: 10000 });
    const cards = () =>
      browser.execute(() => ({
        cards: document.querySelectorAll('[data-testid="message-card"]').length,
        stacked: document.querySelectorAll('[data-testid="thread-entries"], [data-testid="thread-entry"]').length,
        from: document.querySelector('[data-testid="reading-from"]')?.textContent ?? null,
      }));
    const row = await cards();
    report.steps.rowOpens = row;
    assert.equal(row.cards, 1);
    assert.equal(row.stacked, 0);
    report.screenshots.push(await screenshot("conversations-reading-row"));
    await setTheme("dark");
    report.screenshots.push(await screenshot("conversations-reading-row-dark"));
    await setTheme("light");

    const child = await browser.execute(() => document.querySelectorAll('[data-testid="child-row"]')[1]?.id ?? null);
    assert.ok(child, "no second message row");
    await (await browser.$(`[id="${child}"]`)).click();
    await browser.waitUntil(async () => (await cursor())?.id === child, { timeoutMsg: "the message row did not take the cursor" });
    await browser.waitUntil(async () => (await cards()).from !== null && (await cards()).from !== row.from, {
      timeout: 10000,
      timeoutMsg: "the pane did not switch to the message row's message",
    });
    const own = await cards();
    report.steps.childOpens = own;
    assert.equal(own.cards, 1);
    assert.equal(own.stacked, 0);
    report.screenshots.push(await screenshot("conversations-reading-child"));
  });

  it("works from the keyboard: → unfolds and enters, ↓ walks, ← goes up and folds", async () => {
    // Folded with the mouse, then the conversation row takes the cursor.
    await (await browser.$(`[id="${conversationId}"] [data-testid="row-replies"]`)).click();
    await waitExpanded("false");
    await waitChildren(false);
    await (await browser.$(`[id="${conversationId}"]`)).click();
    await browser.waitUntil(async () => (await cursor())?.id === conversationId, { timeoutMsg: "the row did not take the cursor" });

    await browser.keys("ArrowRight");
    await waitExpanded("true");
    await waitChildren(true);
    await browser.keys("ArrowRight");
    await browser.waitUntil(async () => (await cursor())?.level === "2", { timeoutMsg: "→ did not enter the conversation" });
    const entered = await cursor();
    assert.equal(entered.posinset, "1");
    assert.equal(entered.selected, "true");
    await browser.keys("ArrowDown");
    await browser.keys("ArrowDown");
    await browser.waitUntil(async () => (await cursor())?.posinset === "3", { timeoutMsg: "↓ did not walk the messages" });
    report.screenshots.push(await screenshot("conversations-keyboard"));
    await browser.keys("ArrowLeft");
    await browser.waitUntil(async () => (await cursor())?.id === conversationId, { timeoutMsg: "← did not go up to the conversation" });
    await browser.keys("ArrowLeft");
    await waitExpanded("false");
    await waitChildren(false);
    report.steps.keyboard = { entered, after: await cursor() };
    assert.deepEqual(await cspViolations(), [], "CSP violations");
  });

  it("scrolls smoothly with the conversation unfolded", async () => {
    await (await browser.$(`[id="${conversationId}"] [data-testid="row-replies"]`)).click();
    await waitExpanded("true");
    await waitChildren(true);
    await browser.execute(() => {
      const list = document.querySelector('[data-testid="message-list"]');
      if (list) list.scrollTop = 0;
    });
    const measured = await measureScroll({ durationMs: 4000, pxPerFrame: 80 });
    assert.ok(!measured.error, measured.error);
    const s = measured.summary;
    report.scroll = { ...s, largestGaps: [...measured.gaps].sort((a, b) => b - a).slice(0, 8) };
    assert.ok(s.frames > 100, `only ${s.frames} frames`);
    assert.ok(s.frameMedianMs <= 17, `median frame ${s.frameMedianMs} ms`);
    assert.ok(s.frameMaxMs <= 50, `longest frame ${s.frameMaxMs} ms`);
    assert.ok(s.longestBlockingMs <= 50, `main thread blocked for ${s.longestBlockingMs} ms`);
    assert.equal(s.blankFrames, 0, `blank frames: ${JSON.stringify(measured.blankFrames.slice(0, 5))}`);
    report.steps.account = account.email;
  });
});
