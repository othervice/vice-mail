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
// 5. The look, measured in the wide list and in the narrow one beside the reading pane (after the
//    screenshot pass of 24.11.2026): the row shows the conversation's subject ("Planning the 2026
//    offsite", not the newest "AW: …"); the replies pill gives way before the subject and never
//    pushes the date; the dotted line leaves from under the conversation's avatar and runs on to
//    the last message; child rows size the sender to its name and give the text the rest. With a
//    second account in the unified inbox, the account label gives way to the names first, down to
//    its colour dot.
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

const BOB = { email: "bob@vicemail.test", password: "bob-test-pw" };

const require = createRequire(import.meta.url);
const AXE_SOURCE = fs.readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");

const report = { screenshots: [], steps: {} };
let account;
let conversationId = null;
/** The theme and language before this spec, put back at its end. */
let preferencesBefore = null;

/** Light or dark through the core, as a11y.e2e.mjs does: the list keeps its state meanwhile. */
async function setTheme(theme) {
  await coreCall("set_preferences", { preferences: { ...preferencesBefore, language: "en", theme } });
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

/**
 * A screenshot that leaves the keyboard where it was: `screenshot` blurs the focused element (no
 * focus ring in the picture), so the next keys would go to the page body (as in keyboard.e2e.mjs).
 */
async function shot(name) {
  await browser.execute(() => {
    window.__e2eFocus = document.activeElement;
  });
  const file = await screenshot(name);
  await browser.execute(() => window.__e2eFocus?.focus());
  return file;
}

/**
 * Where the parts of a conversation row and its child rows are (CSS pixels): what the layout
 * rules of the screenshot pass promise.
 */
const layout = (id) =>
  browser.execute((rowId) => {
    const row = document.getElementById(rowId);
    const box = (el) => {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width };
    };
    // Cut, by any fraction of a pixel: an ellipsis shows as soon as the text is wider than its box.
    const cut = (el) => {
      if (!el) return false;
      const range = document.createRange();
      range.selectNodeContents(el);
      return range.getBoundingClientRect().width > el.getBoundingClientRect().width + 0.05;
    };
    const subject = row?.querySelector('[data-testid="row-subject"]');
    const pill = row?.querySelector('[data-testid="row-replies"]');
    const avatar = row?.querySelector('[data-testid="avatar"]');
    return {
      narrow: Boolean(row?.querySelector(".flex-col")),
      row: box(row),
      subject: subject?.textContent ?? null,
      subjectCut: cut(subject),
      subjectBox: box(subject),
      pill: box(pill),
      word: box(pill?.querySelector('[data-testid="row-replies-word"]')),
      time: box(row?.querySelector("time")),
      avatar: box(avatar),
      line: box(row?.querySelector('[data-testid="tree-line"]')),
      children: [...document.querySelectorAll('[data-testid="child-row"]')].map((child) => {
        const sender = child.querySelector('[data-testid="child-sender"]');
        const text = child.querySelector('[data-testid="child-text"]');
        return {
          // The conversation's last message (the list renders only some of them).
          last: child.getAttribute("aria-posinset") === child.getAttribute("aria-setsize"),
          row: box(child),
          sender: box(sender),
          senderNeeds: sender?.scrollWidth ?? 0,
          senderCut: cut(sender),
          text: box(text),
          textCut: cut(text),
          group: box(text?.parentElement),
          line: box(child.querySelector('[data-testid="tree-line"]')),
          avatar: box(child.querySelector('[data-testid="avatar"]')),
        };
      }),
    };
  }, id);

/** The promises of the screenshot pass, for the row `id` unfolded (see the header, point 5). */
function checkLayout(found, where) {
  const near = (a, b, slack = 1) => Math.abs(a - b) <= slack;
  // The conversation's subject: its oldest message's, without the prefix.
  assert.equal(found.subject, "Planning the 2026 offsite", where);
  // The pill gives way first: the subject is cut only once the pill's word is gone.
  assert.ok(!found.subjectCut || found.word.width <= 1, `${where}: subject cut while the pill keeps ${String(found.word.width)} px of its word`);
  assert.ok(found.pill.right <= found.row.right, `${where}: the pill leaves the row`);
  // The date stays whole and in the row; on the pill's line it comes after it.
  assert.ok(found.time.right <= found.row.right + 0.5, `${where}: the date is pushed out`);
  if (near(found.time.top, found.pill.top, 4)) assert.ok(found.pill.right <= found.time.left, `${where}: the pill reaches the date`);
  // The line: from under the avatar (its start covered by it) down to the row's end, at its centre.
  assert.ok(found.line, `${where}: no line in the conversation's row`);
  assert.ok(found.line.top >= found.avatar.top && found.line.top < found.avatar.bottom, `${where}: the line starts at ${String(found.line.top)}, the avatar spans ${String(found.avatar.top)}–${String(found.avatar.bottom)}`);
  assert.ok(near(found.line.bottom, found.row.bottom), `${where}: the line ends before the row's end`);
  assert.ok(near(found.line.left, (found.avatar.left + found.avatar.right) / 2, 1.5), `${where}: the line is not under the avatar's centre`);
  // On through every message: no gap between rows, ending in the middle of the last one.
  const children = found.children;
  assert.ok(children.length > 1, `${where}: no messages below the row`);
  children.forEach((child, index) => {
    const last = child.last;
    assert.ok(near(child.line.top, child.row.top), `${where}: a gap above message ${String(index + 1)}`);
    assert.ok(near(child.line.left, found.line.left), `${where}: the line bends at message ${String(index + 1)}`);
    if (last) assert.ok(near(child.line.bottom, (child.row.top + child.row.bottom) / 2, 1), `${where}: the line does not end in the last message`);
    else assert.ok(near(child.line.bottom, child.row.bottom), `${where}: the line stops in message ${String(index + 1)}`);
    // The sender as wide as its name (at most a third), the text takes the rest of the line.
    assert.ok(child.sender.width <= child.senderNeeds + 1, `${where}: message ${String(index + 1)}'s sender is wider than its name`);
    if (child.senderCut) assert.ok(near(child.sender.width, child.group.width / 3, 2), `${where}: message ${String(index + 1)}'s sender is cut below a third`);
    assert.ok(near(child.text.right, child.group.right), `${where}: message ${String(index + 1)}'s text leaves room unused`);
  });
}

/**
 * Narrows the message list to `width` CSS pixels (through the CSSOM, as the CSP allows; null puts
 * it back), so the rules that share a line are measured under pressure too.
 */
async function narrowList(width) {
  await browser.execute((w) => {
    const list = document.querySelector('[data-testid="message-list"]');
    if (list) list.style.maxWidth = w === null ? "" : `${String(w)}px`;
  }, width);
  // Layout follows at once (every measurement forces it; the rows are as wide as the list). No
  // animation frames to wait for: a window started from a terminal session gets none.
  await browser.pause(100);
}

/**
 * Stage 18 (task F, decision 0054): beside the reading pane the list keeps its reading width, open
 * message or not; the wide one-line layout needs a list over 720 px. Widened like the drag handle
 * does it (the size variable on the box of list and pane, through the CSSOM), with the pane's
 * minimum lifted for the measurement; `false` puts both back.
 */
async function wideList(on) {
  await browser.execute((wide) => {
    const list = document.querySelector('[data-testid="message-list"]');
    const column = list?.parentElement;
    const box = column?.parentElement;
    if (!column || !box) return;
    if (wide) {
      box.style.setProperty("--ui-list-width-reading", "50rem");
      column.style.maxWidth = "none";
    } else {
      box.style.removeProperty("--ui-list-width-reading");
      column.style.maxWidth = "";
    }
  }, on);
  // The rows follow the list's width (its ResizeObserver): one line above 720 px.
  await browser.waitUntil(
    () => browser.execute((wide) => (document.querySelector('[data-testid="message-list"] [data-testid="message-row"] .flex-col') === null) === wide, on),
    { timeout: 5000, timeoutMsg: on ? "the widened list kept narrow rows" : "the list did not get narrow again" },
  );
}

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
    // English labels ("59 replies", "Sent") whatever the system's language; put back at the end.
    await coreCall("set_preferences", { preferences: { ...preferencesBefore, language: "en" } });
    await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.lang)) === "en", {
      timeoutMsg: "the app did not switch to English",
    });
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
          // Conversations with replies; the offsite thread if it is rendered ("the offsite" is a
          // topic of single seed messages too).
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
    await wideList(true);
    const wide = await layout(conversationId);
    report.steps.layoutWide = { narrow: wide.narrow, subject: wide.subject, subjectCut: wide.subjectCut, word: wide.word, line: wide.line, avatar: wide.avatar };
    assert.equal(wide.narrow, false, "the list wider than 720 px is wide");
    checkLayout(wide, "wide list");
    await setTheme("light");
    report.screenshots.push(await screenshot("conversations-unfolded-light"));
    const lightFindings = await runAxe();
    assert.deepEqual(lightFindings, [], `axe, light: ${JSON.stringify(lightFindings)}`);
    await setTheme("dark");
    report.screenshots.push(await screenshot("conversations-unfolded-dark"));
    const darkFindings = await runAxe();
    assert.deepEqual(darkFindings, [], `axe, dark: ${JSON.stringify(darkFindings)}`);
    await setTheme("light");
    await wideList(false);
  });

  it("shows one message on the right: the conversation row one, a message row its own", async () => {
    // At its reading width again (the step before widened it), also if that step failed.
    await wideList(false);
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
    // Beside the reading pane the list is narrow: the same promises, two lines a row.
    await browser.waitUntil(async () => (await layout(conversationId)).narrow, { timeoutMsg: "the list beside the reading pane is not narrow" });
    const narrow = await layout(conversationId);
    report.steps.layoutNarrow = {
      subject: narrow.subject,
      subjectCut: narrow.subjectCut,
      word: narrow.word,
      pill: narrow.pill,
      line: narrow.line,
      avatar: narrow.avatar,
      children: narrow.children.slice(0, 4).map((c) => ({ sender: c.sender?.width, needs: c.senderNeeds, textCut: c.textCut })),
    };
    checkLayout(narrow, "narrow list");
    // Narrower still: the pill gives up its word, then the subject is cut (both measured).
    report.steps.layoutSqueezed = [];
    for (const width of [380, 330, 290]) {
      await narrowList(width);
      const squeezed = await layout(conversationId);
      report.steps.layoutSqueezed.push({ width, subjectCut: squeezed.subjectCut, word: squeezed.word.width, pill: squeezed.pill.width });
      checkLayout(squeezed, `list of ${String(width)} px`);
    }
    await narrowList(null);
    const squeezes = report.steps.layoutSqueezed;
    assert.ok(squeezes.some((m) => m.word > 1 && m.word < 40 && !m.subjectCut), `the pill's word never gave way alone: ${JSON.stringify(squeezes)}`);
    assert.ok(squeezes.some((m) => m.subjectCut && m.word <= 1), `the subject was never cut after the word: ${JSON.stringify(squeezes)}`);
    await setTheme("light");
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
    report.screenshots.push(await shot("conversations-keyboard"));
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

  it("puts the names before the account label in the unified inbox, down to its colour dot", async () => {
    await coreCall("add_account", {
      settings: {
        email: BOB.email,
        displayName: "Bob Baker",
        imap: { host: SERVER.host, port: SERVER.imapsPort, security: "tls", username: BOB.email },
        smtp: { host: SERVER.host, port: SERVER.smtpsPort, security: "tls", username: BOB.email },
      },
      password: BOB.password,
    });
    await browser.waitUntil(
      async () => (await coreCall("list_accounts")).every((a) => a.status.state === "idle" && a.status.lastSyncAt !== null),
      { timeout: 240000, interval: 500, timeoutMsg: "bob's first sync did not finish" },
    );
    const unified = await browser.$('[data-testid="unified-tree"] [role="treeitem"]');
    await unified.waitForExist({ timeout: 20000 });
    await unified.click();
    await browser.waitUntil(async () => (await unified.getAttribute("aria-current")) === "page", { timeoutMsg: "the unified inbox did not open" });
    await browser.waitUntil(async () => (await browser.execute(() => document.querySelectorAll('[data-testid="message-list"] [data-testid="row-account"]').length)) > 0, {
      timeout: 20000,
      timeoutMsg: "no account labels in the unified inbox",
    });
    // A message open on the right: the list is narrow, names and label share the first line.
    await (await browser.$('[data-testid="message-list"] [data-testid="message-row"]')).click();
    await testId("message-card").waitForExist({ timeout: 10000 });
    const measure = () =>
      browser.execute(() =>
        [...document.querySelectorAll('[data-testid="message-list"] [data-testid="message-row"]')].flatMap((row) => {
          const sender = row.querySelector('[data-testid="row-sender"]');
          const label = row.querySelector('[data-testid="row-account"]');
          const dot = label?.querySelector('[data-testid="row-account-dot"]');
          if (!sender || !label || !dot) return [];
          // Cut by any fraction of a pixel (an ellipsis shows then).
          const cut = (el) => {
            const range = document.createRange();
            range.selectNodeContents(el);
            return range.getBoundingClientRect().width > el.getBoundingClientRect().width + 0.05;
          };
          const name = label.querySelector('[data-testid="row-account-name"]');
          const box = label.getBoundingClientRect();
          return [
            {
              names: sender.textContent,
              namesCut: cut(sender),
              // The label's name on its visible line, and whether it is cut there.
              nameShown: Boolean(name && name.getBoundingClientRect().top < box.bottom - 1),
              nameCut: Boolean(name && cut(name)),
              labelMax: Number.parseFloat(getComputedStyle(label).maxWidth),
              label: label.getAttribute("title"),
              labelWidth: label.getBoundingClientRect().width,
              dotWidth: dot.getBoundingClientRect().width,
              rowName: row.querySelector('[data-testid="row-name"]')?.textContent ?? "",
            },
          ];
        }),
      );
    await browser.waitUntil(async () => (await measure()).length > 0, { timeoutMsg: "no rows with names and an account label" });
    const check = (rows) => {
      for (const row of rows) {
        assert.ok(["Alice Archer", "Bob Baker"].includes(row.label ?? ""), `label ${String(row.label)}`);
        assert.ok(row.rowName.includes(`Account: ${String(row.label)}`), `the row's name lacks the account: ${row.rowName}`);
        // Names cut only once the label is down to its dot.
        if (row.namesCut) assert.ok(row.labelWidth <= row.dotWidth + 1, `"${String(row.names)}" cut beside a label of ${String(row.labelWidth)} px`);
        // The account's name whole or not at all; cut only when longer than the label's width.
        if (row.nameShown && row.nameCut) assert.ok(row.labelWidth >= row.labelMax - 1, `"${String(row.label)}" cut to a fragment in ${String(row.labelWidth)} px`);
      }
    };
    const rows = await measure();
    report.steps.unifiedLabels = rows.slice(0, 8);
    check(rows);
    // Narrower: labels give way, then names are cut beside a bare dot (both measured).
    await narrowList(280);
    const squeezed = await measure();
    report.steps.unifiedSqueezed = squeezed.slice(0, 8);
    check(squeezed);
    assert.ok(
      squeezed.some((row) => row.labelWidth <= row.dotWidth + 1),
      `no label went down to its dot: ${JSON.stringify(squeezed.map((r) => r.labelWidth))}`,
    );
    assert.ok(squeezed.some((row) => !row.nameShown), "no label hid its name");
    report.screenshots.push(await screenshot("conversations-unified-squeezed"));
    await narrowList(null);
    report.screenshots.push(await screenshot("conversations-unified-labels"));
    const findings = await runAxe();
    assert.deepEqual(findings, [], `axe, unified inbox: ${JSON.stringify(findings)}`);
  });
});
