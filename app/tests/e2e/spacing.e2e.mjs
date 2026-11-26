// Stage 21, item 4 (decision 0071): the spacing pass in the real app. Every main view in light and
// dark, at 1280×800 and at the narrowest supported window (900×600), as a screenshot, and measured
// against the rules that make it look finished (docs/DESIGN.md, "Abstände"):
//
// - header-height: the bars at the top of the window (view headers, the column's top, the
//   composer's and the AI panel's header) are `--ui-header-height` high;
// - bars-align: the bars right under the header that stand side by side (the list's quick filter
//   or table header, the reading pane's toolbar) end on the same line; beside the reading pane at
//   the list's default width the quick filter is one row;
// - equal-heights: the controls of one bar (buttons, icon buttons, chips, fields, selects) are
//   equally high and, in a bar of one row, share their middle;
// - field-height: framed fields and selects are 32 px;
// - icon-gap: 8 px between the icon and the label of a button, menu entry, navigation row or
//   palette entry, 4 px in round chips and small buttons with 12 px text;
// - edges: what stands in one column starts at one inset (the list's title, section labels and
//   the rows' mark column, stage 22: no avatars; the column's section labels and row icons; the palette's group labels and icons; the
//   activity view and its header);
// - clipped: no text is cut off without an ellipsis, chips and pills never (the replies pill's
//   word keeps at least 2.5 em or goes), and no text runs past its card, menu or dialog.
//
// Scenes: the inbox (comfortable, compact), a conversation unfolded, a bundle unfolded, the
// reading pane with attachments and its end (quick reply), a notice, the context menu, search,
// the command palette, the views dialog, the composer, eight settings sections, the AI panel,
// contacts, activity and outbox; the update question once, if it shows. Every rule is checked in
// every scene; a scene's failures are collected and fail its test at the end, so the screenshots
// of a failing run are still complete. Sets up alice (read-only) and bob itself and removes them
// at the end; appends a conversation and a message with attachments to alice's INBOX and one
// message to bob's (`Spacing <run>`); turns AI on for one scene with a cloud profile without a key
// (nothing is sent) and removes it again. Leaves theme, language, density, window size and the
// column as it found them. Screenshots go to tests/e2e/screenshots/spacing-*.png, the
// measurements to spacing-report.json.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { $, browser } from "@wdio/globals";
import { attach, coreCall, cspViolations, openView, SCREENSHOT_DIR, setConversations, setSidebar, testId } from "./support/app.mjs";
import { ALICE, SERVER, appendMessages, appendToBob } from "./support/testmail.mjs";

const BOB = { email: "bob@vicemail.test", password: "bob-test-pw" };
const RUN = Date.now().toString(36);
const MARK = `Spacing ${RUN}`;
const MOD = process.platform === "darwin" ? "Meta" : "Control";
const SIZES = [
  { width: 1280, height: 800 },
  { width: 900, height: 600 },
];
const THEMES = ["light", "dark"];
const report = { run: RUN, scenes: [], failures: [], notes: [] };
let alice = null;
let bob = null;
let restore = null;

// ---------------------------------------------------------------------------------------------
// Test data

function eml({ subject, from, to, index, id, parent, attachments = [] }) {
  const head = [
    `Date: ${new Date(Date.now() - (10 - index) * 60000).toUTCString().replace("GMT", "+0000")}`,
    `From: ${from}`,
    `To: ${to}`,
    `Subject: ${subject}`,
    `Message-ID: <${id}@spacing.vicemail.test>`,
    ...(parent ? [`In-Reply-To: <${parent}@spacing.vicemail.test>`, `References: <${parent}@spacing.vicemail.test>`] : []),
    "MIME-Version: 1.0",
  ];
  const text = `Hello,\r\n\r\nthis is ${subject}. The spacing pass looks at every view of the app.\r\n\r\nRegards\r\n`;
  if (attachments.length === 0) return [...head, "Content-Type: text/plain; charset=utf-8", "", text, ""].join("\r\n");
  const boundary = `b-${id}`;
  const parts = [
    `--${boundary}`,
    "Content-Type: text/plain; charset=utf-8",
    "",
    text,
    ...attachments.flatMap((a) => [
      `--${boundary}`,
      `Content-Type: ${a.type}; name="${a.name}"`,
      `Content-Disposition: attachment; filename="${a.name}"`,
      "Content-Transfer-Encoding: base64",
      "",
      Buffer.from(a.body).toString("base64"),
    ]),
    `--${boundary}--`,
    "",
  ];
  return [...head, `Content-Type: multipart/mixed; boundary="${boundary}"`, "", ...parts].join("\r\n");
}

function writeMessages(messages) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "spacing-"));
  return messages.map((message, index) => {
    const file = path.join(dir, `m${String(index)}.eml`);
    fs.writeFileSync(file, eml({ ...message, index }));
    return file;
  });
}

const ALICE_TO = "Alice Archer <alice@vicemail.test>";
const MESSAGES = [
  { subject: `${MARK} review`, from: "Grace Hopper <grace@example.org>", to: ALICE_TO, id: `c1-${RUN}` },
  { subject: `Re: ${MARK} review`, from: "Ada Lovelace <ada@example.org>", to: ALICE_TO, id: `c2-${RUN}`, parent: `c1-${RUN}` },
  { subject: `Re: ${MARK} review`, from: "Grace Hopper <grace@example.org>", to: ALICE_TO, id: `c3-${RUN}`, parent: `c2-${RUN}` },
  {
    subject: `${MARK} attachments`,
    from: "Edsger Dijkstra <edsger@example.org>",
    to: ALICE_TO,
    id: `a1-${RUN}`,
    attachments: [
      { name: "Quarterly report.pdf", type: "application/pdf", body: "%PDF-1.4\n% spacing test\n" },
      { name: "Notes for the meeting with a long name.txt", type: "text/plain", body: "Notes\n" },
    ],
  },
];

// ---------------------------------------------------------------------------------------------
// The measurements, taken in the page

const PROBE = () => {
  const rootStyle = getComputedStyle(document.documentElement);
  const rem = Number.parseFloat(rootStyle.fontSize);
  const length = (name) => {
    const value = rootStyle.getPropertyValue(name).trim();
    return value.endsWith("rem") ? Number.parseFloat(value) * rem : Number.parseFloat(value);
  };
  const round = (v) => Math.round(v * 10) / 10;
  const box = (r) => ({ left: round(r.left), top: round(r.top), right: round(r.right), bottom: round(r.bottom), width: round(r.width), height: round(r.height) });
  const transparent = (el) => {
    for (let at = el; at && at !== document.body; at = at.parentElement) if (Number(getComputedStyle(at).opacity) === 0) return true;
    return false;
  };
  /** Rendered, not hidden, and at least partly inside the window and every clipping ancestor. */
  const shown = (el) => {
    if (!el.isConnected || el.closest("[inert], [hidden], [data-e2e-mark]")) return false;
    const style = getComputedStyle(el);
    if (style.display === "none" || style.visibility === "hidden") return false;
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0 || r.bottom <= 0 || r.right <= 0 || r.top >= innerHeight || r.left >= innerWidth) return false;
    for (let at = el.parentElement; at && at !== document.body; at = at.parentElement) {
      const s = getComputedStyle(at);
      if (s.overflowX === "visible" && s.overflowY === "visible") continue;
      const a = at.getBoundingClientRect();
      if (r.bottom <= a.top || r.top >= a.bottom || r.right <= a.left || r.left >= a.right) return false;
    }
    return !transparent(el);
  };
  const where = (el) => el.closest("[data-testid]")?.getAttribute("data-testid") ?? el.tagName.toLowerCase();
  const label = (el) => (el.getAttribute("aria-label") || el.textContent || "").trim().replace(/\s+/g, " ").slice(0, 48);

  // Fields and selects: their frame (a field inside a framed label or box counts as the frame).
  const framed = (input) => {
    for (let at = input; at && at !== document.body; at = at.parentElement) {
      if (Number.parseFloat(getComputedStyle(at).borderTopWidth) > 0) return at;
      if (at !== input && at.querySelectorAll("input, select, textarea").length > 1) break;
    }
    return input;
  };
  // Bars: the top of each pane and the bars right under the header.
  const CONTROL = 'button, select, input:not([type="checkbox"]):not([type="radio"]):not([type="hidden"]), [role="combobox"]';
  const controlsIn = (bar) => {
    const title = bar.matches("header") ? bar.querySelector("h1")?.parentElement : null;
    const seen = new Set();
    const controls = [];
    // A text field shows as its frame; what sits inside a frame (a clear button) is part of it.
    const frames = [...bar.querySelectorAll("input:not([type=checkbox]):not([type=radio]):not([type=hidden])")].map(framed);
    for (const element of bar.querySelectorAll(CONTROL)) {
      if (title?.contains(element)) continue;
      const control = element.matches("input") ? framed(element) : element;
      if (!element.matches("input") && frames.some((frame) => frame !== element && frame.contains(element))) continue;
      if (seen.has(control) || !shown(control) || control.parentElement?.closest(CONTROL)) continue;
      seen.add(control);
      const r = control.getBoundingClientRect();
      controls.push({ label: label(control), testid: control.getAttribute("data-testid"), height: round(r.height), middle: round(r.top + r.height / 2) });
    }
    return controls;
  };
  const barName = (el) => {
    if (el.matches("header")) return `header ${el.querySelector("h1")?.textContent?.trim() ?? where(el)}`;
    if (el.parentElement?.matches('[data-testid="reading-pane"]')) return "reading toolbar";
    if (el.parentElement?.matches('[data-testid="sidebar-column"]')) return "column top";
    if (el.parentElement?.matches('[data-testid="search-bar"]')) return "search bar";
    return where(el);
  };
  const bars = [
    ...[...document.querySelectorAll("header")].filter((header) => !header.closest("article, [data-testid='message-card']")),
    ...document.querySelectorAll('[data-testid="sidebar-column"] > div:first-child'),
    ...document.querySelectorAll('[data-testid="reading-pane"] > div:first-child'),
    ...document.querySelectorAll('[data-testid="quick-filter"], [data-testid="table-header"], [data-testid="selection-bar"], [role="toolbar"]'),
    ...document.querySelectorAll('[data-testid="search-bar"] > div:first-child'),
  ]
    .filter(shown)
    .map((el) => ({ name: barName(el), ...box(el.getBoundingClientRect()), controls: controlsIn(el) }));

  const fields = [...document.querySelectorAll('select, input:is([type="text"], [type="email"], [type="search"], [type="password"], [type="number"], [type="url"], [type="datetime-local"], :not([type]))')]
    .filter((el) => shown(el))
    .map((el) => {
      const frame = framed(el);
      const r = frame.getBoundingClientRect();
      const s = getComputedStyle(frame);
      return { where: where(el), label: el.getAttribute("aria-label") ?? el.getAttribute("placeholder") ?? el.id, tag: el.tagName.toLowerCase(), height: round(r.height), bordered: Number.parseFloat(s.borderTopWidth) > 0, border: s.borderTopColor, background: s.backgroundColor };
    });

  // Icon and label: the first shown icon of a control and the first text after it.
  const HOSTS =
    'button, a[href], label, [role="menuitem"], [role="menuitemradio"], [role="menuitemcheckbox"], [role="option"], [role="tab"], [role="treeitem"], [data-testid="row-replies"]';
  const usedIcons = new Set();
  const gaps = [];
  for (const host of document.querySelectorAll(HOSTS)) {
    if (!shown(host)) continue;
    // Rows of the message list start with their mark column, not an icon; the accent swatches
    // are colour, not icons.
    if (host.closest('[data-testid="message-list"]') && !host.matches('[data-testid="row-replies"]')) continue;
    if (host.closest('[data-testid^="accent-"]')) continue;
    const icon = [...host.querySelectorAll("svg")].find((svg) => !usedIcons.has(svg) && shown(svg));
    if (!icon) continue;
    const walker = document.createTreeWalker(host, NodeFilter.SHOW_TEXT);
    let text = null;
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      if (!(node.textContent ?? "").trim() || !node.parentElement || !shown(node.parentElement)) continue;
      if (node.parentElement.closest(".sr-only")) continue;
      if (icon.compareDocumentPosition(node) & Node.DOCUMENT_POSITION_FOLLOWING) {
        text = node;
        break;
      }
    }
    if (!text) continue;
    const range = document.createRange();
    range.selectNodeContents(text);
    const t = range.getClientRects()[0];
    const i = icon.getBoundingClientRect();
    if (!t || t.top > i.bottom || t.bottom < i.top) continue;
    const gap = round(t.left - i.right);
    if (gap < -0.5 || gap > 24) continue;
    usedIcons.add(icon);
    const h = host.getBoundingClientRect();
    const radius = Number.parseFloat(getComputedStyle(host).borderTopLeftRadius);
    const role = host.getAttribute("role");
    const kind = role?.startsWith("menuitem")
      ? "menu"
      : role === "option"
        ? "option"
        : radius >= h.height / 2 - 0.5
          ? "chip"
          : role === "tab" || role === "treeitem" || host.closest('[role="tree"], [role="tablist"]')
            ? "nav"
            : h.height <= 24.5 || Number.parseFloat(getComputedStyle(host).fontSize) <= 12.5 * (rem / 16)
              ? "small"
              : "button";
    gaps.push({ where: where(host), label: label(host), kind, height: round(h.height), icon: round(i.width), gap });
  }

  // Text cut off: the text of an element against the nearest ancestor that clips.
  const clipped = [];
  const seenClip = new Set();
  for (const el of document.body.querySelectorAll("*")) {
    const own = [...el.childNodes].filter((n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? "").trim());
    if (own.length === 0 || el.closest(".sr-only, svg, script, style") || !shown(el)) continue;
    const range = document.createRange();
    range.selectNodeContents(el);
    const t = range.getBoundingClientRect();
    if (t.width <= 0) continue;
    for (let at = el; at && at !== document.body; at = at.parentElement) {
      const s = getComputedStyle(at);
      if (s.overflowX === "visible") continue;
      const a = at.getBoundingClientRect();
      const cut = t.right > a.right + 1 || t.left < a.left - 1;
      if (cut && !seenClip.has(at)) {
        seenClip.add(at);
        const chip = Boolean(el.closest('[data-testid="row-replies"], [data-testid^="quick-filter-"], .rounded-full'));
        clipped.push({
          where: where(el),
          text: (el.textContent ?? "").trim().slice(0, 40),
          ellipsis: s.textOverflow === "ellipsis",
          chip,
          by: at.tagName.toLowerCase(),
          width: round(a.width),
          em: Number.parseFloat(s.fontSize),
          hidden: getComputedStyle(el).visibility === "hidden",
        });
      }
      break;
    }
  }

  // Text that runs past the edge of its card, menu or dialog (not clipped, but out of its box).
  const outside = [];
  for (const el of document.body.querySelectorAll("*")) {
    const own = [...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? "").trim());
    if (!own || el.closest(".sr-only, svg") || !shown(el)) continue;
    const box = el.closest('.rounded-card, [role="menu"], [role="dialog"], [data-testid="notice"]');
    if (!box || box === el) continue;
    // Cut by an element on the way (truncated names): that is the clip rule's business.
    let clipping = false;
    for (let at = el; at && at !== box; at = at.parentElement) if (getComputedStyle(at).overflowX !== "visible") clipping = true;
    if (clipping) continue;
    const range = document.createRange();
    range.selectNodeContents(el);
    const t = range.getBoundingClientRect();
    const b = box.getBoundingClientRect();
    if (t.width > 0 && (t.right > b.right + 1 || t.left < b.left - 1)) outside.push({ where: where(el), text: (el.textContent ?? "").trim().slice(0, 40), by: round(t.right - b.right) });
  }

  // Edges of what stands in one column.
  const leftOf = (selector, root = document) => {
    const el = [...root.querySelectorAll(selector)].find(shown);
    return el ? round(el.getBoundingClientRect().left) : null;
  };
  const textLeftOf = (selector, root = document) => {
    const el = [...root.querySelectorAll(selector)].find(shown);
    if (!el) return null;
    const range = document.createRange();
    range.selectNodeContents(el);
    return round(range.getBoundingClientRect().left);
  };
  const list = document.querySelector('[data-testid="message-list"]');
  const column = document.querySelector('[data-testid="sidebar-column"]');
  const header = document.querySelector("main header");
  const defaultInset = header ? Math.abs(Number.parseFloat(getComputedStyle(header).paddingInlineStart) - length("--ui-space-6")) < 0.5 : false;
  const activity = document.querySelector('[data-testid="activity-view"]');
  const edges = {
    // The title's inset is larger where the macOS window buttons reach into the header.
    listTitle: list && shown(list) && defaultInset ? textLeftOf("main header h1") : null,
    viewTitle: defaultInset ? textLeftOf("main header h1") : null,
    activityStart: activity && shown(activity) ? leftOf(':scope > div > *, :scope > *', activity) : null,
    listSection: list ? textLeftOf('[data-testid="list-section"] span, [data-testid="list-section"]', list) : null,
    // A row of the list itself, not one inside a card (priority, an unfolded bundle). Stage 22: the
    // mark column (unread dot, selection box) where the avatar was.
    rowMark: list
      ? (() => {
          const mark = [...list.querySelectorAll('[data-testid="message-row"] [data-testid="row-mark"], [data-testid="table-row"] [data-testid="row-mark"]')].find(
            (el) => !el.closest(".list-card") && shown(el),
          );
          return mark ? round(mark.getBoundingClientRect().left) : null;
        })()
      : null,
    rowAvatars: list ? list.querySelectorAll('[data-testid="avatar"]').length : 0,
    columnSection: column ? textLeftOf('[data-testid^="sidebar-section-"] > span', column) : null,
    columnRowIcon: column ? leftOf('[role="treeitem"] svg', column) : null,
    paletteGroup: textLeftOf('[data-testid="command-palette"] [role="group"] > [role="presentation"]'),
    paletteIcon: leftOf('[data-testid="command-palette"] [role="option"] svg'),
  };

  return {
    window: { width: innerWidth, height: innerHeight },
    scale: rem / 16,
    theme: document.documentElement.dataset.theme,
    tokens: { header: length("--ui-header-height") },
    scrollsSideways: document.documentElement.scrollWidth > document.documentElement.clientWidth,
    bars,
    gaps,
    fields,
    clipped,
    outside,
    edges,
  };
};

// ---------------------------------------------------------------------------------------------
// The rules, checked in Node on the measurements

const near = (a, b, tolerance = 0.6) => a !== null && b !== null && Math.abs(a - b) <= tolerance;

/** Allowed gaps between icon and label, per kind of control (in px at 100 % text). */
const GAPS = { button: [8], chip: [4], small: [4], menu: [8], option: [8], nav: [8] };

function rules(scene, facts) {
  const failures = [];
  const fail = (rule, detail) => failures.push({ scene, rule, detail });
  const s = facts.scale;
  if (facts.scrollsSideways) fail("layout", "the window scrolls sideways");
  // header-height: every bar at the top of the window.
  for (const bar of facts.bars) {
    const top = bar.top <= 1 && (bar.name.startsWith("header") || bar.name === "column top");
    if (top && !near(bar.height, facts.tokens.header)) fail("header-height", `${bar.name}: ${String(bar.height)} px, not ${String(facts.tokens.header)}`);
  }
  // bars-align: the bars right under a view header that stand side by side end on one line.
  const header = facts.bars.find((b) => b.name.startsWith("header") && b.top <= 1);
  if (header) {
    // Where the list is narrower than its default beside the pane (the AI panel takes room too),
    // the quick filter may wrap; at the default width and above it is one row.
    const under = facts.bars.filter(
      (b) =>
        near(b.top, header.bottom, 1.5) &&
        ["quick-filter", "table-header", "reading toolbar", "selection-bar"].includes(b.name) &&
        !(b.name === "quick-filter" && b.width < 383 * s),
    );
    const bottoms = [...new Set(under.map((b) => b.bottom))];
    if (under.length > 1 && Math.max(...bottoms) - Math.min(...bottoms) > 0.6) {
      fail("bars-align", under.map((b) => `${b.name} ${String(b.top)}–${String(b.bottom)}`).join(", "));
    }
  }
  // equal-heights: the controls of a bar.
  for (const bar of facts.bars) {
    if (bar.controls.length < 2) continue;
    const heights = bar.controls.map((c) => c.height);
    const middles = bar.controls.map((c) => c.middle);
    // A bar of one row: as high as its tallest control and some padding (the rail and the
    // column's foot stack their controls; a wrapped quick filter has rows of its own).
    const oneRow = bar.height < Math.max(...heights) * 1.9;
    if (Math.max(...heights) - Math.min(...heights) > 0.6) {
      fail("equal-heights", `${bar.name}: ${bar.controls.map((c) => `${c.testid ?? c.label} ${String(c.height)}`).join(", ")}`);
    } else if (oneRow && Math.max(...middles) - Math.min(...middles) > 1) {
      fail("equal-heights", `${bar.name}: middles ${JSON.stringify(middles)}`);
    }
  }
  // field-height: framed fields and selects are 32 px (docs/DESIGN.md, "Felder, Auswahllisten").
  for (const f of facts.fields) {
    if (f.bordered && f.height < 100 && !near(f.height, 32 * s)) fail("field-height", `${f.where} (${f.tag}): ${String(f.height)} px`);
  }
  // icon-gap.
  for (const g of facts.gaps) {
    const allowed = GAPS[g.kind].map((v) => v * s);
    if (!allowed.some((v) => near(g.gap, v, 0.75))) fail("icon-gap", `${g.kind} ${g.where} "${g.label}": ${String(g.gap)} px (allowed ${allowed.join(" or ")})`);
  }
  // clipped.
  for (const c of facts.clipped) {
    if (c.hidden) continue;
    // The replies pill gives its word up first ("3 repl…", decision 0049), but never shows a
    // sliver of it (decision 0071: at least 2.5 em, else nothing).
    const word = c.where === "row-replies-word" && c.ellipsis && c.width >= 2.5 * c.em - 0.5;
    if (!word && (!c.ellipsis || c.chip)) fail("clipped", `${c.where} "${c.text}"${c.ellipsis ? ` (a chip cut to ${String(c.width)} px)` : ""}, by ${c.by}`);
  }
  for (const o of facts.outside) fail("clipped", `${o.where} "${o.text}" runs ${String(o.by)} px past its box`);
  // edges.
  const e = facts.edges;
  const same = (what, a, b) => {
    if (a !== null && b !== null && !near(a, b, 1)) fail("edges", `${what}: ${String(a)} vs ${String(b)}`);
  };
  same("list title vs row mark", e.listTitle, e.rowMark);
  same("list section vs row mark", e.listSection, e.rowMark);
  if (e.rowAvatars > 0) fail("edges", `${String(e.rowAvatars)} avatars in the list (decision 0072)`);
  same("column section vs row icon", e.columnSection, e.columnRowIcon);
  same("activity title vs content", e.viewTitle, e.activityStart);
  same("palette group vs option icon", e.paletteGroup, e.paletteIcon);
  return failures;
}

/**
 * Measures the current state, takes its screenshot and records what broke a rule. Menus, the
 * palette and dialogs keep the focus (they close when it leaves); elsewhere it is taken away, so
 * no focus ring of the last synthetic click is in the picture.
 */
async function capture(name, { keepFocus = false } = {}) {
  await browser.execute((keep) => {
    if (!keep && document.activeElement instanceof HTMLElement) document.activeElement.blur();
    for (const animation of document.getAnimations()) animation.finish();
  }, keepFocus);
  await browser.pause(150);
  const facts = await browser.execute(PROBE);
  const scene = `${name}-${String(facts.theme)}-${String(facts.window.width)}x${String(facts.window.height)}`;
  const failures = rules(scene, facts);
  report.scenes.push({ scene, facts, failures });
  report.failures.push(...failures);
  fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
  await browser.saveScreenshot(path.join(SCREENSHOT_DIR, `spacing-${scene}.png`));
  return failures;
}

// ---------------------------------------------------------------------------------------------
// Getting to each view

async function setTheme(theme) {
  await coreCall("set_preferences", { preferences: { theme, language: "en" } });
  await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme, { timeoutMsg: `no ${theme} theme` });
}

async function setSize({ width, height }) {
  await browser.setWindowSize(width, height);
  await browser.waitUntil(async () => {
    const size = await browser.execute(() => ({ width: innerWidth, height: innerHeight }));
    return size.width === width && size.height === height;
  }, { timeout: 10000, timeoutMsg: `the window is not ${String(width)}×${String(height)}` });
  await browser.pause(300);
}

async function paletteCommand(text) {
  await browser.keys([MOD, "k"]);
  await testId("command-palette").waitForExist();
  await browser.keys([...text]);
  await browser.keys("Enter");
  await testId("command-palette").waitForExist({ reverse: true });
}

async function closeOverlays() {
  for (let i = 0; i < 3; i += 1) {
    const open = await browser.execute(() => Boolean(document.querySelector('[role="menu"], [role="dialog"], [data-testid="command-palette"]')));
    if (!open) return;
    await browser.keys("Escape");
    await browser.pause(150);
  }
}

/** alice's inbox from her tree, in the column or (narrow window) the rail's folders. */
async function openAliceInbox() {
  await closeOverlays();
  const clicked = await browser.execute(() => {
    const tree = [...document.querySelectorAll('[role="tree"]')].find((t) => (t.getAttribute("aria-label") ?? "").includes("Alice"));
    const inbox = tree?.querySelector('[data-folder-role="inbox"]');
    inbox?.click();
    return Boolean(inbox);
  });
  if (!clicked) {
    // Narrow: the column is folded into the rail; its inbox is the first account's (alice's).
    await testId("rail-inbox").click();
  }
  await testId("message-list").waitForExist({ timeout: 20000 });
  await browser.pause(400);
}

const rowNames = () =>
  browser.execute(() =>
    [...document.querySelectorAll('[data-testid="message-list"] :is([role="option"], [role="treeitem"])')].map((o) => ({
      id: o.id,
      name: (o.querySelector('[data-testid="row-name"]')?.textContent ?? o.textContent ?? "").trim(),
    })),
  );

async function scrollList(to) {
  await browser.execute((top) => {
    const list = document.querySelector('[data-testid="message-list"]');
    if (!list) return;
    list.scrollTop = top;
    list.dispatchEvent(new Event("scroll"));
  }, to);
  await browser.pause(200);
}

/** Scrolls the list until a row matches, puts it in view and returns its id. */
async function revealRow(pattern, steps = 80) {
  await scrollList(0);
  for (let i = 0; i < steps; i += 1) {
    const found = (await rowNames()).find((row) => pattern.test(row.name));
    if (found) {
      await browser.execute((id) => document.getElementById(id)?.scrollIntoView({ block: "center" }), found.id);
      await browser.pause(200);
      return found.id;
    }
    const atEnd = await browser.execute(() => {
      const list = document.querySelector('[data-testid="message-list"]');
      const before = list.scrollTop;
      list.scrollTop += list.clientHeight * 0.8;
      list.dispatchEvent(new Event("scroll"));
      return list.scrollTop === before;
    });
    await browser.pause(80);
    if (atEnd) break;
  }
  return null;
}

/** One real mouse event at a point of the viewport, as context-menu.e2e.mjs sends them. */
async function mouse(kind, point) {
  const viewportHeight = await browser.execute(() => innerHeight);
  return coreCall("e2e_native_mouse_event", { kind, x: point.x, y: point.y, viewportHeight });
}

async function rowBox(id) {
  return browser.execute((rowId) => {
    const r = document.getElementById(rowId)?.getBoundingClientRect();
    return r ? { left: r.left, top: r.top, width: r.width, height: r.height } : null;
  }, id);
}

/**
 * AI on with one cloud profile for alice (as readme-shots.e2e.mjs), or off and the profile gone.
 * The switch itself is flipped in Settings → AI, as a user would: the UI's availability follows
 * its own settings changes only. Turning AI on sends nothing (ai.e2e.mjs).
 */
async function enableAi(on) {
  const flip = async (want) => {
    await openSettings("ai");
    const toggle = await testId("ai-enabled");
    if ((await toggle.getAttribute("aria-checked")) !== String(want)) await toggle.click();
    await browser.waitUntil(async () => (await coreCall("get_ai_settings")).enabled === want, { timeoutMsg: `AI did not turn ${want ? "on" : "off"}` });
  };
  if (!on) {
    const hide = await $('button[aria-label="Hide AI assistant"]');
    if (await hide.isExisting()) await hide.click();
    if ((await coreCall("get_ai_settings").catch(() => ({ enabled: false }))).enabled) await flip(false).catch(() => undefined);
    for (const profile of (await coreCall("get_ai_settings").catch(() => ({ profiles: [] }))).profiles) {
      await coreCall("delete_ai_profile", { profileId: profile.id }).catch(() => undefined);
    }
    return;
  }
  const settings = await coreCall("get_ai_settings");
  const preset = settings.presets.find((p) => p.kind === "anthropic") ?? settings.presets[0];
  const saved = await coreCall("save_ai_profile", {
    profile: { id: null, name: preset.name, kind: preset.kind, baseUrl: preset.baseUrl, model: "test-model", structuredOutput: true, maxOutputTokens: null, key: { type: "keep" } },
  });
  const profileId = saved.profiles[0].id;
  for (const aiFunction of ["summary", "reply_suggestion", "writing_help", "search"]) await coreCall("set_ai_function", { aiFunction, profileId });
  await coreCall("set_ai_account_cloud", { accountId: alice.id, allowed: true });
  await flip(true);
}

async function openSettings(section) {
  await closeOverlays();
  await testId("rail-settings").click();
  const tab = testId(`settings-tab-${section}`);
  await tab.waitForExist({ timeout: 10000 });
  await tab.click();
  await browser.waitUntil(async () => (await tab.getAttribute("data-state")) === "active", { timeoutMsg: `settings ${section} did not open` });
  await browser.pause(300);
}

// ---------------------------------------------------------------------------------------------

/** The scenes of one theme and window size; each returns its failures. */
async function scenes() {
  const failures = [];
  const add = async (name, run, options = {}) => {
    try {
      await run();
      failures.push(...(await capture(name, options)));
    } catch (e) {
      const detail = String(e?.message ?? e).split("\n")[0];
      report.notes.push(`${name}: ${detail}`);
      failures.push({ scene: name, rule: "scene", detail });
    } finally {
      await closeOverlays().catch(() => undefined);
    }
  };

  // The inbox with its top message (decision 0054), comfortable and compact rows.
  await add("inbox", async () => {
    await openAliceInbox();
    await scrollList(0);
  });
  await add("inbox-compact", async () => {
    await paletteCommand("Use compact rows");
    await openAliceInbox();
    await scrollList(0);
  });
  await paletteCommand("Use comfortable rows");

  // A conversation unfolded, one of its messages open.
  await add("conversation", async () => {
    await openAliceInbox();
    const id = await revealRow(new RegExp(`${MARK} review`));
    assert.ok(id, "no conversation row");
    const expanded = await browser.execute((rowId) => document.getElementById(rowId)?.getAttribute("aria-expanded"), id);
    if (expanded !== "true") await $(`[id="${id}"] [data-testid="row-replies"]`).click();
    await browser.waitUntil(async () => (await browser.execute(() => document.querySelectorAll('[data-testid="child-row"]').length)) >= 3, {
      timeout: 10000,
      timeoutMsg: "the conversation did not unfold",
    });
    const child = await browser.execute(() => [...document.querySelectorAll('[data-testid="child-row"]')][1]?.id ?? null);
    if (child) await $(`[id="${child}"]`).click();
    await browser.pause(600);
  });

  // A bundle unfolded (the seed's newsletters or notifications).
  await add("bundle", async () => {
    await openAliceInbox();
    const id = await revealRow(/^(Newsletters|Notifications)\b/, 40);
    assert.ok(id, "no bundle in the inbox");
    const open = await browser.execute((rowId) => document.getElementById(rowId)?.querySelector('[data-testid="bundle-row"]')?.dataset.expanded, id);
    if (open !== "true") await $(`[id="${id}"]`).click();
    await browser.pause(500);
    await browser.execute((rowId) => document.getElementById(rowId)?.scrollIntoView({ block: "start" }), id);
    await browser.pause(300);
  });

  // The reading pane: header card, attachments, quick reply.
  await add("reading", async () => {
    await openAliceInbox();
    const id = await revealRow(new RegExp(`${MARK} attachments`));
    assert.ok(id, "no message with attachments");
    await $(`[id="${id}"]`).click();
    await testId("reading-subject").waitForExist({ timeout: 20000 });
    await browser.waitUntil(() => browser.execute(() => /Quarterly report/.test(document.querySelector('[data-testid="reading-pane"]')?.textContent ?? "")), {
      timeout: 20000,
      timeoutMsg: "the attachments did not show",
    });
    await browser.pause(500);
  });
  // The same pane scrolled to its end: the quick reply.
  await add("reading-end", async () => {
    await browser.execute(() => {
      const scroller = document.querySelector('[data-testid="reading-scroller"]');
      if (scroller) scroller.scrollTop = scroller.scrollHeight;
    });
  });

  // A read-only notice: the archive shortcut on alice's read-only message.
  await add("notice", async () => {
    await testId("message-list").click();
    await browser.keys("e");
    await testId("notice").waitForExist({ timeout: 5000 });
  });
  // Gone again, so it does not cover the scenes after it.
  await browser.execute(() => {
    for (const button of document.querySelectorAll('[data-testid="notice"] button[aria-label="Dismiss"]')) button.click();
  });
  await testId("notice").waitForExist({ reverse: true, timeout: 5000 }).catch(() => undefined);

  // The context menu of a row.
  await add("context-menu", async () => {
    await openAliceInbox();
    const id = await revealRow(new RegExp(`${MARK} attachments`));
    const row = await rowBox(id);
    const point = { x: row.left + row.width * 0.5, y: row.top + row.height / 2 };
    await mouse("move", point);
    await mouse("rightClick", point);
    await browser.waitUntil(() => browser.execute(() => Boolean(document.querySelector('[data-testid="message-menu"] [role="menu"]'))), {
      timeout: 10000,
      timeoutMsg: "no context menu",
    });
    await browser.pause(200);
  }, { keepFocus: true });
  await mouse("move", { x: 2, y: 2 }).catch(() => undefined);

  // Search: the bar over the results, a result open.
  await add("search", async () => {
    await openAliceInbox();
    await browser.keys([MOD, "f"]);
    const input = await testId("search-input");
    await input.waitForExist();
    await input.setValue(MARK);
    await browser.waitUntil(async () => (await browser.execute(() => document.querySelectorAll('[data-testid="message-list"] :is([role="option"], [role="treeitem"])').length)) > 0, {
      timeout: 15000,
      timeoutMsg: "no search results",
    });
    await browser.pause(600);
  });
  if (await testId("search-close").isExisting()) await testId("search-close").click().catch(() => undefined);

  // The command palette.
  await add("palette", async () => {
    await browser.keys([MOD, "k"]);
    await testId("command-palette").waitForExist();
    await browser.keys([..."go"]);
    await browser.pause(300);
  }, { keepFocus: true });

  // The column's views dialog.
  await add("dialog", async () => {
    await paletteCommand("Edit views");
    await browser.waitUntil(() => browser.execute(() => Boolean(document.querySelector('[role="dialog"]'))), { timeout: 5000, timeoutMsg: "no dialog" });
  }, { keepFocus: true });

  // The composer.
  await add("composer", async () => {
    await openAliceInbox();
    await $('button[aria-label="New message"]').click();
    await testId("composer-subject").waitForExist({ timeout: 20000 });
    await browser.pause(500);
  });
  if (await testId("composer-discard").isExisting()) {
    await testId("composer-discard").click();
    const confirm = await testId("composer-confirm-discard");
    if (await confirm.isExisting()) await confirm.$("button=Discard").click();
  }
  await testId("composer").waitForExist({ reverse: true, timeout: 10000 }).catch(() => undefined);

  // Settings: the sections with forms.
  for (const section of ["general", "accounts", "reading", "compose", "appearance", "keyboard", "notifications", "about"]) {
    await add(`settings-${section}`, async () => {
      await openSettings(section);
      if (section === "accounts") {
        const entry = await testId(`account-row-${alice.id}`);
        if (await entry.isExisting()) {
          await entry.click();
          await browser.pause(500);
        }
      }
    });
  }

  // The AI panel beside the inbox (a cloud profile without a key: nothing is ever sent).
  await add("ai-panel", async () => {
    await enableAi(true);
    await openAliceInbox();
    // The panel starts unfolded the first time; later it is as the last scene left it (folded).
    await browser.waitUntil(async () => (await testId("ai-panel").isExisting()) || (await $('button[aria-label="Show AI assistant"]').isExisting()), {
      timeout: 15000,
      timeoutMsg: "neither the AI panel nor its button",
    });
    if (!(await testId("ai-panel").isExisting())) await $('button[aria-label="Show AI assistant"]').click();
    await testId("ai-panel").waitForExist({ timeout: 10000 });
    await browser.pause(400);
  });
  await enableAi(false);

  // Contacts, activity, outbox.
  await add("contacts", async () => {
    await closeOverlays();
    await openView("contacts");
    await browser.pause(800);
  });
  await add("activity", async () => {
    await testId("rail-activity").click();
    await browser.pause(500);
  });
  await add("outbox", async () => {
    await testId("rail-outbox").click();
    await browser.pause(500);
  });
  return failures;
}

describe("stage 21: the spacing pass over every view", () => {
  before(async () => {
    await attach();
    const preferences = await coreCall("get_preferences");
    const sidebar = (await testId("rail-folders").getAttribute("aria-expanded")) === "true";
    restore = {
      theme: preferences.theme,
      language: preferences.language,
      reading: await coreCall("get_reading_preferences"),
      sidebar,
      size: await browser.execute(() => ({ width: innerWidth, height: innerHeight })),
      conversations: (await coreCall("get_ui_preferences")).conversations,
    };
    // The conversation scene needs conversations on (the default); earlier specs of the full run
    // may have left them off.
    await setConversations(true);
    await setTheme("light");
    await appendMessages("INBOX", writeMessages(MESSAGES));
    await appendToBob(writeMessages([{ subject: `${MARK} for bob`, from: "Grace Hopper <grace@example.org>", to: "Bob Baker <bob@vicemail.test>", id: `b1-${RUN}` }]));
    const accountSettings = (who, name) => ({
      email: who.email,
      displayName: name,
      imap: { host: SERVER.host, port: SERVER.imapsPort, security: "tls", username: who.email },
      smtp: { host: SERVER.host, port: SERVER.smtpsPort, security: "tls", username: who.email },
    });
    alice = await coreCall("add_account", { settings: accountSettings(ALICE, "Alice Archer"), password: ALICE.password });
    bob = await coreCall("add_account", { settings: accountSettings(BOB, "Bob Baker"), password: BOB.password });
    // Stage 22 (decision 0078): a new account shows its address until it is named in the lists.
    await coreCall("rename_account", { accountId: alice.id, name: "Alice Archer" });
    await coreCall("rename_account", { accountId: bob.id, name: "Bob Baker" });
    await browser.waitUntil(
      async () => {
        const accounts = await coreCall("list_accounts");
        return accounts.length === 2 && accounts.every((a) => a.status.state === "idle" && a.status.lastSyncAt !== null);
      },
      { timeout: 300000, interval: 500, timeoutMsg: "the first syncs did not finish" },
    );
    await coreCall("set_reading_preferences", { preferences: { ...restore.reading, markReadOnOpen: "never" } });
    const no = await $("button=No, thanks");
    if (await no.isExisting()) {
      // The update question (decision 0037), as it shows on a fresh data directory.
      await setSize(SIZES[0]);
      await setSidebar(true);
      await capture("update-question");
      await no.click();
    }
    await setSidebar(true);
  });

  after(async () => {
    await closeOverlays().catch(() => undefined);
    await paletteCommand("Use comfortable rows").catch(() => undefined);
    await enableAi(false);
    if (restore) {
      await coreCall("set_preferences", { preferences: { theme: restore.theme, language: restore.language } }).catch(() => undefined);
      await coreCall("set_reading_preferences", { preferences: restore.reading }).catch(() => undefined);
      await setSize(restore.size).catch(() => undefined);
      await setSidebar(restore.sidebar).catch(() => undefined);
      await setConversations(restore.conversations).catch(() => undefined);
    }
    for (const account of [alice, bob]) {
      if (account) await coreCall("remove_account", { accountId: account.id }).catch((e) => console.log(`cleanup failed: ${e}`));
    }
    fs.mkdirSync(SCREENSHOT_DIR, { recursive: true });
    fs.writeFileSync(path.join(SCREENSHOT_DIR, "spacing-report.json"), JSON.stringify(report, null, 2));
    const counts = {};
    for (const f of report.failures) counts[f.rule] = (counts[f.rule] ?? 0) + 1;
    console.log(`\nE2E REPORT spacing: ${String(report.scenes.length)} scenes, failures by rule ${JSON.stringify(counts)}\n`);
  });

  for (const size of SIZES) {
    for (const theme of THEMES) {
      it(`${String(size.width)}×${String(size.height)}, ${theme}`, async function () {
        this.timeout(900000);
        await setSize(size);
        await setTheme(theme);
        if (size.width >= 1280) await setSidebar(true);
        const failures = await scenes();
        assert.deepEqual(
          failures.map((f) => `${f.scene} ${f.rule}: ${f.detail}`),
          [],
        );
      });
    }
  }

  it("leaves no CSP violation", async () => {
    assert.deepEqual(await cspViolations(), []);
  });
});
